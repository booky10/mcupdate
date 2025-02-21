use reqwest::blocking::Client;
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fs, thread};

const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const MINECRAFT_ICON_URL: &str =
    "https://resources.download.minecraft.net/df/df274fe57c49ef1af6d218703d805db76a5c8af9";

const CACHE_FILE: &str = "prev_mc_snapshot.txt";
const UPDATE_INTERVAL: Duration = Duration::from_secs(3 * 60);

const NTFY_HOST: &str = "https://ntfy.sh/";
const NTFY_TOPIC: &str = "mcupdate";
const NTFY_ICON: &str = MINECRAFT_ICON_URL;

const DC_WEBHOOK_NAME: &str = "Minecraft Update";
const DC_WEBHOOK_ICON: &str = MINECRAFT_ICON_URL;

fn fetch_json(client: &Client, url: &str) -> Option<Value> {
    if let Ok(response) = client.get(url).send() {
        if let Ok(text) = response.text() {
            return serde_json::from_str(&text).ok();
        }
    }
    None
}

fn check_minecraft_update(
    client: &Client,
    discord_webhook_url: &str,
    healthchecks_url: &Option<String>,
) {
    if let Some(healthchecks_url) = healthchecks_url {
        client.get(healthchecks_url).send().unwrap();
    }

    let manifest_json = fetch_json(client, &MANIFEST_URL);
    if manifest_json.is_none() {
        println!("Received invalid response while requesting manifest url");
        return;
    }
    let manifest_json = manifest_json.unwrap();

    let latest_snapshot = manifest_json["latest"]["snapshot"].as_str().unwrap();
    if fs::exists(CACHE_FILE).unwrap_or(false) {
        // if this has been executed before, check nothing changed
        let prev_snapshot = fs::read_to_string(CACHE_FILE).unwrap();
        if latest_snapshot == prev_snapshot {
            println!("Previous snapshot is still latest snapshot: {}", prev_snapshot);
            return;
        }
    }

    let latest_data_url = manifest_json["versions"][0]["url"].as_str().unwrap();
    let latest_data_json = fetch_json(client, latest_data_url).unwrap();
    let release_time_str = latest_data_json["releaseTime"].as_str().unwrap();
    let version_type = latest_data_json["type"].as_str().unwrap();

    println!(
        "Encountered new Minecraft {} {} (released at {})",
        version_type, latest_snapshot, release_time_str,
    );

    let release_time_secs = chrono::DateTime::parse_from_rfc3339(release_time_str)
        .ok()
        .unwrap()
        .timestamp();
    let current_time_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .unwrap()
        .as_secs() as i64;
    let release_diff_hours = (current_time_secs - release_time_secs) / 60 / 60;
    let release_diff_string = format!(
        "{} hour{} ago",
        release_diff_hours,
        if release_diff_hours == 1 { "" } else { "s" },
    );

    let response = client
        .post(NTFY_HOST)
        .header("Icon", NTFY_ICON)
        .json(&serde_json::json!({
            "topic": NTFY_TOPIC,
            "message": format!("{} {} {}", version_type, latest_snapshot, release_diff_string),
            "title": format!("New Minecraft {}", version_type),
            "tags": ["minecraft", "update", "snapshot"],
            "priority": 4,
            "click": latest_data_url,
        }))
        .send()
        .unwrap();
    println!(
        "Posted to ntfy.sh, received code {}",
        response.status().as_str(),
    );

    let response = client
        .post(discord_webhook_url)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "embeds": [
                {
                    "title": format!("New Minecraft {}: {}", version_type, latest_snapshot),
                    "timestamp": release_time_str,
                    "color": 16776960,
                    "footer": {
                        "text": DC_WEBHOOK_NAME,
                        "icon_url": DC_WEBHOOK_ICON
                    }
                }
            ]
        }))
        .send()
        .unwrap();
    println!(
        "Posted to Discord Webhook, received code {}",
        response.status().as_str(),
    );

    fs::write(CACHE_FILE, latest_snapshot).ok().unwrap();
}

fn main() {
    let discord_webhook_url = std::env::var("DISCORD_WEBHOOK_URL")
        .expect("DISCORD_WEBHOOK_URL not set");
    let healthchecks_url = std::env::var("HEALTHCHECKS_URL").ok();

    let scheduler = thread::spawn(move || {
        let client = &Client::new();
        loop {
            check_minecraft_update(client, &discord_webhook_url, &healthchecks_url);
            thread::sleep(UPDATE_INTERVAL);
        }
    });
    scheduler.join().unwrap();
}
