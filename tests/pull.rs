#[tokio::test]
async fn pull() {
    exp::docker_runner::pull_image("busybox", "latest").await.unwrap();
}
