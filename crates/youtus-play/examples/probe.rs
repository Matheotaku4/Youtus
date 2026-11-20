//! Diagnostic : charge une URL via libmpv et imprime durée/position.
//! `cargo run -p youtus-play --example probe -- <url> [user-agent]`

use std::time::Duration;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let url = args.next().expect("URL en argument");
    let user_agent = args.next().unwrap_or_default();

    let player = youtus_play::MpvPlayer::new()?;
    let mut headers = Vec::new();
    if !user_agent.is_empty() {
        headers.push(("User-Agent".to_owned(), user_agent));
        headers.push(("Sec-Fetch-Mode".to_owned(), "no-cors".to_owned()));
    }
    player.load(&url, &headers)?;

    for _ in 0..20 {
        std::thread::sleep(Duration::from_secs(1));
        println!(
            "pos={:.3} dur={:.3} eof={} paused={}",
            player.position(),
            player.duration(),
            player.eof_reached(),
            player.paused()
        );
    }
    Ok(())
}
