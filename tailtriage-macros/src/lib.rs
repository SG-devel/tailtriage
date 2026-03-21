use proc_macro::TokenStream;

use quote::quote;
use syn::parse::{Parse, ParseStream, Parser};
use syn::{
    parse_macro_input, punctuated::Punctuated, Error, Expr, ExprLit, FnArg, ItemFn, Lit, Meta,
    MetaList, MetaNameValue, Result as SynResult, Token,
};

#[derive(Default)]
struct InstrumentArgs {
    route: Option<Expr>,
    kind: Option<Expr>,
    tailtriage: Option<Expr>,
    request_id: Option<Expr>,
    skip: Option<Punctuated<syn::Ident, Token![,]>>,
}

impl Parse for InstrumentArgs {
    fn parse(input: ParseStream<'_>) -> SynResult<Self> {
        let metas = Punctuated::<Meta, Token![,]>::parse_terminated(input)?;
        let mut args = InstrumentArgs::default();

        for meta in metas {
            match meta {
                Meta::NameValue(MetaNameValue { path, value, .. }) if path.is_ident("route") => {
                    args.route = Some(value);
                }
                Meta::NameValue(MetaNameValue { path, value, .. }) if path.is_ident("kind") => {
                    args.kind = Some(value);
                }
                Meta::NameValue(MetaNameValue { path, value, .. })
                    if path.is_ident("tailtriage") =>
                {
                    args.tailtriage = Some(value);
                }
                Meta::NameValue(MetaNameValue { path, value, .. })
                    if path.is_ident("request_id") =>
                {
                    args.request_id = Some(value);
                }
                Meta::List(MetaList { path, tokens, .. }) if path.is_ident("skip") => {
                    if args.skip.is_some() {
                        return Err(Error::new_spanned(path, "duplicate skip argument"));
                    }

                    let parsed =
                        Punctuated::<syn::Ident, Token![,]>::parse_terminated.parse2(tokens)?;
                    args.skip = Some(parsed);
                }
                _ => {
                    return Err(Error::new_spanned(
                        meta,
                        "unsupported argument; expected route = <expr>, kind = <expr>, tailtriage = <expr>, request_id = <expr>, or skip(...)",
                    ));
                }
            }
        }

        Ok(args)
    }
}

#[proc_macro_attribute]
pub fn instrument_request(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as InstrumentArgs);
    let input_fn = parse_macro_input!(item as ItemFn);

    match expand_instrument_request(args, input_fn) {
        Ok(expanded) => expanded.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[allow(clippy::too_many_lines)]
fn expand_instrument_request(
    args: InstrumentArgs,
    mut input_fn: ItemFn,
) -> SynResult<proc_macro2::TokenStream> {
    if input_fn.sig.asyncness.is_none() {
        return Err(Error::new_spanned(
            input_fn.sig.fn_token,
            "#[instrument_request] only supports async functions",
        ));
    }

    let skip_names = args.skip.unwrap_or_default();
    validate_skipped_args(&skip_names, &input_fn)?;

    let route_expr = args
        .route
        .unwrap_or_else(|| default_route_expr(&input_fn.sig.ident));
    let kind_expr = args
        .kind
        .unwrap_or_else(|| default_kind_expr(&input_fn.sig.ident));
    let tailtriage_expr = args.tailtriage;
    let request_id_expr = args.request_id;

    let route_field = make_field_expr("route", route_expr);
    let kind_field = make_field_expr("kind", kind_expr);

    let skip_attr = if skip_names.is_empty() {
        quote! {}
    } else {
        quote! { skip(#skip_names), }
    };

    let body = input_fn.block;
    let unix_time_ms_expr = unix_time_ms_expr();
    let returns_result = returns_result(&input_fn.sig.output);
    let (outcome_expr, tail_event) = if returns_result {
        (
            quote! {
                let __tailtriage_outcome = match &__tailtriage_result {
                    Ok(_) => "ok",
                    Err(_) => "error",
                };
            },
            quote! {
                if __tailtriage_outcome == "ok" {
                    ::tracing::info!(
                        target: "tailtriage::request",
                        route = __tailtriage_route,
                        kind = __tailtriage_kind,
                        outcome = __tailtriage_outcome,
                        duration_us = __tailtriage_duration_us,
                        "request completed"
                    );
                } else {
                    ::tracing::warn!(
                        target: "tailtriage::request",
                        route = __tailtriage_route,
                        kind = __tailtriage_kind,
                        outcome = __tailtriage_outcome,
                        duration_us = __tailtriage_duration_us,
                        "request completed"
                    );
                }
            },
        )
    } else {
        (
            quote! {
                let __tailtriage_outcome = "ok";
            },
            quote! {
                ::tracing::info!(
                    target: "tailtriage::request",
                    route = __tailtriage_route,
                    kind = __tailtriage_kind,
                    outcome = __tailtriage_outcome,
                    duration_us = __tailtriage_duration_us,
                    "request completed"
                );
            },
        )
    };
    let record_request = if let Some(tailtriage) = tailtriage_expr {
        let request_id = request_id_expr.unwrap_or_else(default_request_id_expr);
        quote! {
            let __tailtriage_outcome_value = match __tailtriage_outcome {
                "ok" => ::tailtriage_core::Outcome::Ok,
                "error" => ::tailtriage_core::Outcome::Error,
                "timeout" => ::tailtriage_core::Outcome::Timeout,
                "cancelled" => ::tailtriage_core::Outcome::Cancelled,
                "rejected" => ::tailtriage_core::Outcome::Rejected,
                other => ::tailtriage_core::Outcome::Other(other.to_string()),
            };
            (#tailtriage)
                .request_with(
                    __tailtriage_route.clone(),
                    ::tailtriage_core::RequestOptions::new().request_id(#request_id),
                )
                .with_kind(__tailtriage_kind.clone())
                .complete(__tailtriage_outcome_value);
        }
    } else {
        quote! {}
    };

    input_fn.block = Box::new(syn::parse_quote!({
        let __tailtriage_route = ::std::string::ToString::to_string(&#route_field);
        let __tailtriage_kind = ::std::string::ToString::to_string(&#kind_field);
        let __tailtriage_unix_time_ms = #unix_time_ms_expr;
        let __tailtriage_started_at_unix_ms = __tailtriage_unix_time_ms();
        let __tailtriage_started = ::std::time::Instant::now();
        let __tailtriage_result = (async move #body).await;
        #outcome_expr
        let __tailtriage_finished_at_unix_ms = __tailtriage_unix_time_ms();
        let __tailtriage_duration_us =
            ::std::convert::TryFrom::try_from(__tailtriage_started.elapsed().as_micros())
                .unwrap_or(u64::MAX);
        #record_request
        #tail_event
        __tailtriage_result
    }));

    input_fn.attrs.push(syn::parse_quote!(
        #[::tracing::instrument(
            name = "tailtriage.request",
            target = "tailtriage::request",
            #skip_attr
            fields(
                route = ::tracing::field::display(&#route_field),
                kind = ::tracing::field::display(&#kind_field)
            )
        )]
    ));

    Ok(quote!(#input_fn))
}

fn make_field_expr(_name: &str, expr: Expr) -> proc_macro2::TokenStream {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(value),
            ..
        }) => quote!(#value),
        other => quote!((#other)),
    }
}

fn default_route_expr(name: &syn::Ident) -> Expr {
    syn::parse_quote!(concat!(module_path!(), "::", stringify!(#name)))
}

fn default_kind_expr(name: &syn::Ident) -> Expr {
    syn::parse_quote!(stringify!(#name))
}

fn default_request_id_expr() -> Expr {
    syn::parse_quote!(format!(
        "{}-{}",
        __tailtriage_route, __tailtriage_started_at_unix_ms
    ))
}

fn unix_time_ms_expr() -> proc_macro2::TokenStream {
    quote!(|| -> u64 {
        match ::std::time::SystemTime::now().duration_since(::std::time::UNIX_EPOCH) {
            Ok(duration) => {
                ::std::convert::TryInto::try_into(duration.as_millis()).unwrap_or(u64::MAX)
            }
            Err(_) => 0,
        }
    })
}

fn validate_skipped_args(
    skip_names: &Punctuated<syn::Ident, Token![,]>,
    func: &ItemFn,
) -> SynResult<()> {
    for ident in skip_names {
        let found = func.sig.inputs.iter().any(|arg| match arg {
            FnArg::Receiver(_) => ident == "self",
            FnArg::Typed(pat_type) => match pat_type.pat.as_ref() {
                syn::Pat::Ident(pat_ident) => pat_ident.ident == *ident,
                _ => false,
            },
        });

        if !found {
            return Err(Error::new_spanned(
                ident,
                format!("skip argument `{ident}` does not match any function parameter"),
            ));
        }
    }

    Ok(())
}

fn returns_result(output: &syn::ReturnType) -> bool {
    let syn::ReturnType::Type(_, ty) = output else {
        return false;
    };

    let syn::Type::Path(type_path) = ty.as_ref() else {
        return false;
    };

    type_path
        .path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "Result")
}
