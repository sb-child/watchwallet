use clap::Parser;
use crossfire::mpmc;
use futures_util::{StreamExt, stream::FuturesUnordered};
use mytm::{TaskHandle, TaskManager, TypedTaskState};
use std::{backtrace::BacktraceStatus, time::Duration};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use watchwallet::{
    cli::Cli,
    config::{Config, EvmConfig},
    evm::{self, EvmEvent},
    output, printer,
};

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
        app_result
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
    if config.evm.is_empty() {
        anyhow::bail!("配置里没有任何 EVM 链");
    }

    // 每条链一个事件通道；事件由独立的 consumer 渲染并投递到全局打印队列。
    let mut handles: Vec<TaskHandle<(), (), anyhow::Error>> = Vec::new();

    let (print_tx, print_rx) = printer::channel(printer::QUEUE_CAPACITY);
    let print_tx = match config.dzserver.as_ref() {
        Some(dzserver) => {
            tracing::info!(url = %dzserver.grpc_url, "启用 dzserver 打印");
            let printer = printer::Printer::connect(&dzserver.grpc_url)?;
            handles.push(tm.spawn_typed(move |_tm, pu, ct| async move {
                pu.update(());
                printer::run(printer, print_rx, ct).await;
                Ok(())
            }));
            Some(print_tx)
        }
        None => {
            tracing::warn!("未配置 dzserver，只渲染打印包，不提交打印");
            None
        }
    };

    for cfg in config.evm.iter().cloned() {
        let listen_cfg = cfg.clone();
        let print_tx = print_tx.clone();
        let (event_tx, event_rx) = mpmc::unbounded_async::<EvmEvent>();
        let event_tx = event_tx.into_async();

        handles.push(tm.spawn_typed(move |_tm, pu, ct| async move {
            pu.update(());
            evm::listen(listen_cfg, ct, event_tx).await?;
            Ok(())
        }));
        handles.push(tm.spawn_typed(move |_tm, pu, ct| async move {
            pu.update(());
            drain_events(cfg, event_rx, print_tx, ct).await?;
            Ok(())
        }));
    }

    let mut pending = handles
        .iter()
        .map(TaskHandle::wait_terminal)
        .collect::<FuturesUnordered<_>>();
    while let Some(state) = tokio::select! {
        _ = root.cancelled() => None,
        state = pending.next() => state,
    } {
        if let Some(err) = task_failure(state) {
            return Err(err);
        }
    }
    // 收到取消信号后等所有任务收尾，main 里还有 10 秒超时兜底。
    while pending.next().await.is_some() {}
    Ok(())
}

/// 把任务终态里的失败原因转成错误。取消和正常结束都返回 [`None`]。
fn task_failure(state: TypedTaskState<(), (), anyhow::Error>) -> Option<anyhow::Error> {
    match state {
        TypedTaskState::Failed(err) => Some(anyhow::anyhow!("任务失败: {err:#}")),
        TypedTaskState::Panicked(msg) => Some(anyhow::anyhow!("任务崩溃: {msg}")),
        _ => None,
    }
}

/// 事件消费者：渲染成打印包，配置了 dzserver 时投递到打印队列。
async fn drain_events(
    cfg: EvmConfig,
    rx: evm::EventRx,
    print: Option<printer::PrintTx>,
    ct: CancellationToken,
) -> anyhow::Result<()> {
    loop {
        let event = tokio::select! {
            _ = ct.cancelled() => break,
            event = rx.recv() => match event {
                Ok(event) => event,
                // 发送端已全部关闭
                Err(_) => break,
            },
        };
        tracing::debug!(chain = %cfg.chain_name, ?event, "收到 EVM 事件");
        match output::evm::render(&event, &cfg) {
            Ok(rendered) => match &print {
                Some(tx) => {
                    let job = printer::PrintJob {
                        label: format!("{} / {}", cfg.chain_name, rendered.template),
                        typst_tar: rendered.tar,
                    };
                    match tx.try_send(job) {
                        Ok(()) => {}
                        Err(crossfire::TrySendError::Full(job)) => tracing::warn!(
                            chain = %cfg.chain_name,
                            label = %job.label,
                            bytes = job.typst_tar.len(),
                            "打印队列已满，丢弃打印任务"
                        ),
                        Err(crossfire::TrySendError::Disconnected(job)) => tracing::warn!(
                            chain = %cfg.chain_name,
                            label = %job.label,
                            "打印队列已关闭，丢弃打印任务"
                        ),
                    }
                }
                None => tracing::info!(
                    chain = %cfg.chain_name,
                    template = rendered.template,
                    bytes = rendered.tar.len(),
                    "渲染打印包"
                ),
            },
            Err(err) => tracing::warn!(chain = %cfg.chain_name, "渲染打印包失败: {err}"),
        }
    }
    Ok(())
}
