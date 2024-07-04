use metrics::counter;

pub const KEY: &str = "metric";

pub async fn do_work_async() {
    loop {
        let mut iter = 0;
        counter!(KEY).increment(1);

        iter += 1;
        if iter % 100 == 0 {
            tokio::task::yield_now().await
        }
    }
}
