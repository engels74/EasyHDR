#![allow(clippy::unwrap_used)]
//! Benchmarks for configuration serialization and deserialization

#![allow(missing_docs)]

use criterion::{Criterion, criterion_group, criterion_main};
use easyhdr::config::models::Win32App;
use easyhdr::config::{AppConfig, MonitoredApp, UserPreferences, WindowState};
use std::hint::black_box;
use std::path::PathBuf;
use uuid::Uuid;

fn create_large_config() -> AppConfig {
    let mut config = AppConfig {
        monitored_apps: Vec::with_capacity(100),
        preferences: UserPreferences {
            auto_start: true,
            monitoring_interval_ms: 1000,
            show_tray_notifications: true,
            show_update_notifications: true,
            auto_open_release_page: false,
            minimize_to_tray_on_minimize: true,
            minimize_to_tray_on_close: false,
            start_minimized_to_tray: false,
            last_update_check_time: 0,
            cached_latest_version: String::new(),
        },
        window_state: WindowState {
            x: 100,
            y: 100,
            width: 800,
            height: 600,
        },
    };

    // Add 100 monitored apps to simulate a large configuration
    for i in 0..100 {
        config.monitored_apps.push(MonitoredApp::Win32(Win32App {
            id: Uuid::new_v4(),
            display_name: format!("Test Application {i}"),
            exe_path: PathBuf::from(format!("C:\\Games\\Game{i}\\game.exe")),
            process_name: format!("game{i}"),
            enabled: i % 2 == 0,
            icon_data: None,
        }));
    }

    config
}

fn bench_config_serialization(c: &mut Criterion) {
    let config = create_large_config();

    c.bench_function("config_serialize", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&config)).unwrap();
            black_box(json);
        });
    });
}

fn bench_config_deserialization(c: &mut Criterion) {
    let config = create_large_config();
    let json = serde_json::to_string(&config).unwrap();

    c.bench_function("config_deserialize", |b| {
        b.iter(|| {
            let deserialized: AppConfig = serde_json::from_str(black_box(&json)).unwrap();
            black_box(deserialized);
        });
    });
}

fn bench_config_round_trip(c: &mut Criterion) {
    let config = create_large_config();

    c.bench_function("config_round_trip", |b| {
        b.iter(|| {
            let json = serde_json::to_string(black_box(&config)).unwrap();
            let deserialized: AppConfig = serde_json::from_str(&json).unwrap();
            black_box(deserialized);
        });
    });
}

criterion_group!(
    benches,
    bench_config_serialization,
    bench_config_deserialization,
    bench_config_round_trip
);
criterion_main!(benches);
