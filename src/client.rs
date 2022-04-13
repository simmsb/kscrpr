use once_cell::sync::OnceCell;

pub fn client() -> &'static reqwest::Client {
    static INSTANCE: OnceCell<reqwest::Client> = OnceCell::new();
    INSTANCE.get_or_init(|| {
        reqwest::ClientBuilder::new()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/96.0.4664.110 Safari/537.36")
            .build()
            .unwrap()
    })
}
