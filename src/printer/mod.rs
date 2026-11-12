//! 打印投递层。
//!
//! 这里只认识待打印的 tar 和打印结果，不认识任何链、模板或 dzserver 的 protobuf 类型。
//! EVM/SVM 的渲染结果由上层转成 [`PrintJob`] 后投递进来。

mod dzserver;

use self::dzserver::JobId;
use crossfire::mpmc;
use snafu::Snafu;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use url::Url;

/// 打印队列容量。
// 约定：队列满时丢弃新任务并打日志，避免打印机长时间离线拖住监听任务。
pub const QUEUE_CAPACITY: usize = 64;

/// 队列里的打印任务。
#[derive(Debug)]
pub struct PrintJob {
    /// 仅用于日志，例如 "Ethereum Mainnet / evm-erc20-send"
    pub label: String,
    /// 可直接提交给 dzserver 的 tar
    pub typst_tar: Vec<u8>,
}

/// 打印任务终态。
#[derive(Debug)]
pub enum PrintOutcome {
    Succeeded,
    Failed(PrintFailure),
    Cancelled,
}

/// 打印失败信息。
#[derive(Debug)]
pub struct PrintFailure {
    pub stage: FailureStage,
    pub reason: FailureReason,
    /// 服务端给的完整描述，只用于日志
    pub message: String,
    /// 设备状态，仅用于展示和排障
    pub printer_status: Option<&'static str>,
}

/// 失败发生在哪个阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureStage {
    Render,
    Connect,
    WaitForPrinter,
    Print,
    Unspecified,
}

impl FailureStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Render => "RENDER",
            Self::Connect => "CONNECT",
            Self::WaitForPrinter => "WAIT_FOR_PRINTER",
            Self::Print => "PRINT",
            Self::Unspecified => "UNSPECIFIED",
        }
    }

    // 约定：只有还没进入打印阶段的失败才能自动重发；PRINT 阶段可能已经出纸。
    fn is_safe_to_retry(self) -> bool {
        matches!(self, Self::Connect | Self::WaitForPrinter)
    }
}

/// 机器可判的失败原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    Render,
    DeviceNotFound,
    DeviceNotReady,
    PrinterStatus,
    PrinterSilent,
    PrintTimeout,
    PrintDidNotStart,
    Io,
    Internal,
    Unspecified,
}

impl FailureReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Render => "RENDER",
            Self::DeviceNotFound => "DEVICE_NOT_FOUND",
            Self::DeviceNotReady => "DEVICE_NOT_READY",
            Self::PrinterStatus => "PRINTER_STATUS",
            Self::PrinterSilent => "PRINTER_SILENT",
            Self::PrintTimeout => "PRINT_TIMEOUT",
            Self::PrintDidNotStart => "PRINT_DID_NOT_START",
            Self::Io => "IO",
            Self::Internal => "INTERNAL",
            Self::Unspecified => "UNSPECIFIED",
        }
    }
}

/// 打印投递错误。
#[derive(Debug, Snafu)]
pub enum PrintError {
    #[snafu(display("打印服务地址无效: {source}"))]
    Endpoint { source: tonic::transport::Error },
    #[snafu(display("提交打印任务失败: {source}"))]
    Submit { source: tonic::Status },
    #[snafu(display("查询打印状态失败: {source}"))]
    Status { source: tonic::Status },
    #[snafu(display("打印任务 id 长度错误: {len}"))]
    BadJobId { len: usize },
    #[snafu(display("打印任务不存在或已被回收"))]
    JobNotFound,
    #[snafu(display("等待打印任务超时"))]
    Timeout,
    #[snafu(display("打印任务已中断"))]
    Interrupted,
}

pub type PrintTx = crossfire::MAsyncTx<mpmc::Array<PrintJob>>;
pub type PrintRx = crossfire::MAsyncRx<mpmc::Array<PrintJob>>;

/// 创建打印队列。
pub fn channel(capacity: usize) -> (PrintTx, PrintRx) {
    mpmc::bounded_async(capacity)
}

/// dzserver 打印客户端。
pub struct Printer {
    client: dzserver::Client,
}

impl Printer {
    /// 创建客户端。连接是惰性的，打印机暂时不在线不影响监听任务启动。
    pub fn connect(url: &Url) -> Result<Self, PrintError> {
        Ok(Self {
            client: dzserver::Client::connect(url)?,
        })
    }

    async fn submit(&mut self, tar: &[u8]) -> Result<JobId, PrintError> {
        self.client.submit(tar).await
    }

    async fn wait(
        &mut self,
        job: JobId,
        ct: &CancellationToken,
    ) -> Result<PrintOutcome, PrintError> {
        self.client.wait(job, ct).await
    }
}

/// 打印任务 worker：串行提交并等待每个任务结束。
pub async fn run(mut printer: Printer, rx: PrintRx, ct: CancellationToken) {
    loop {
        let job = tokio::select! {
            _ = ct.cancelled() => break,
            job = rx.recv() => match job {
                Ok(job) => job,
                // 所有发送端已关闭
                Err(_) => break,
            },
        };
        match print_job(&mut printer, &job, &ct).await {
            Ok(outcome) => log_outcome(&job, &outcome),
            Err(PrintError::Interrupted) => break,
            Err(err) => tracing::warn!(label = %job.label, "打印任务失败: {err}"),
        }
    }
}

const MAX_RETRIES: u32 = 2;
const RETRY_DELAY: Duration = Duration::from_secs(30);

async fn print_job(
    printer: &mut Printer,
    job: &PrintJob,
    ct: &CancellationToken,
) -> Result<PrintOutcome, PrintError> {
    let mut retries = 0;
    loop {
        if ct.is_cancelled() {
            return Err(PrintError::Interrupted);
        }
        let id = printer.submit(&job.typst_tar).await?;
        tracing::info!(
            label = %job.label,
            job = %id,
            bytes = job.typst_tar.len(),
            "提交打印任务"
        );

        match printer.wait(id, ct).await? {
            PrintOutcome::Failed(failure)
                if retries < MAX_RETRIES && failure.stage.is_safe_to_retry() =>
            {
                retries += 1;
                tracing::warn!(
                    label = %job.label,
                    stage = failure.stage.as_str(),
                    reason = failure.reason.as_str(),
                    retry = retries,
                    "打印未进入出纸阶段，稍后重试"
                );
                sleep_or_cancel(RETRY_DELAY, ct).await?;
            }
            outcome => return Ok(outcome),
        }
    }
}

fn log_outcome(job: &PrintJob, outcome: &PrintOutcome) {
    match outcome {
        PrintOutcome::Succeeded => tracing::info!(label = %job.label, "打印完成"),
        PrintOutcome::Cancelled => tracing::info!(label = %job.label, "打印任务被取消"),
        PrintOutcome::Failed(failure) => tracing::warn!(
            label = %job.label,
            stage = failure.stage.as_str(),
            reason = failure.reason.as_str(),
            printer_status = failure.printer_status.unwrap_or(""),
            message = %failure.message,
            "打印失败"
        ),
    }
}

async fn sleep_or_cancel(duration: Duration, ct: &CancellationToken) -> Result<(), PrintError> {
    tokio::select! {
        _ = ct.cancelled() => Err(PrintError::Interrupted),
        _ = tokio::time::sleep(duration) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(stage: FailureStage) -> PrintFailure {
        PrintFailure {
            stage,
            reason: FailureReason::DeviceNotReady,
            message: String::new(),
            printer_status: None,
        }
    }

    #[test]
    fn retry_only_before_printing() {
        assert!(failure(FailureStage::Connect).stage.is_safe_to_retry());
        assert!(
            failure(FailureStage::WaitForPrinter)
                .stage
                .is_safe_to_retry()
        );
        assert!(!failure(FailureStage::Render).stage.is_safe_to_retry());
        assert!(!failure(FailureStage::Print).stage.is_safe_to_retry());
        assert!(!failure(FailureStage::Unspecified).stage.is_safe_to_retry());
    }
}
