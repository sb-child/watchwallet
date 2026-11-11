use alloy::providers::{Provider, ProviderBuilder, WsConnect};
use clap::Parser;
use mytm::TaskManager;
use std::{backtrace::BacktraceStatus, time::Duration};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use watchwallet::{cli::Cli, config::Config};

fn exit_code(result: anyhow::Result<()>) -> std::process::ExitCode {
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(err) => {
            let bt = err.backtrace();
            if bt.status() == BacktraceStatus::Captured {
                error!("{err:#}\n\nStack backtrace:\n{bt}");
            } else {
                error!("{err:#}");
            }
            std::process::ExitCode::FAILURE
        }
    }
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .with_file(true)
        .with_line_number(true)
        .init();
    let parse_config = cli.parse_config();
    let Ok(config) = parse_config else {
        return exit_code(parse_config.map(|_| ()));
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("创建 tokio runtime 失败");
    let result = runtime.block_on(async {
        let tm = TaskManager::new();
        let tm2 = tm.clone();
        let root = CancellationToken::new();
        let sub = root.child_token();

        let app_fut = app(tm, sub, config);
        tokio::pin!(app_fut);

        let app_result = tokio::select! {
            res = &mut app_fut => res,
            _ = shutdown_signal() => {
                info!("shutdown signal received");
                // 发送取消通知
                root.cancel();
                tm2.close();
                // 继续等待 app_fut 在 10 秒内完成收尾工作
                match tokio::time::timeout(Duration::from_secs(10), app_fut).await {
                    Ok(res) => res,
                    Err(_) => {
                        error!("graceful shutdown timed out after 10s");
                        Err(anyhow::anyhow!("graceful shutdown timed out"))
                    }
                }
            }
        };
        return app_result;
    });
    runtime.shutdown_timeout(Duration::from_secs(3));
    exit_code(result)
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("注册 SIGTERM 失败");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

// ---

async fn app(tm: TaskManager, root: CancellationToken, config: Config) -> anyhow::Result<()> {
    // todo
    Ok(())
}
