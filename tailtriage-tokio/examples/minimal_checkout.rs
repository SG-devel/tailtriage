use tailtriage_core::{Outcome, Tailtriage};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tailtriage = Tailtriage::builder("minimal-checkout")
        .output(std::env::temp_dir().join("tailtriage-minimal-checkout.json"))
        .build()?;

    let request = tailtriage.request("/checkout").with_kind("http");

    {
        let _inflight = request.inflight("checkout_inflight");

        request
            .queue("queue_wait")
            .with_depth_at_start(3)
            .await_on(tokio::time::sleep(std::time::Duration::from_millis(5)))
            .await;

        request
            .stage("db_call")
            .await_on(async { Ok::<(), &'static str>(()) })
            .await?;
    }

    request.complete(Outcome::Ok);

    tailtriage.shutdown()?;
    Ok(())
}
