//! dzserver gRPC 客户端。
//!
//! 生成的 protobuf 类型只在这个模块里出现；上层只看到 [`PrintOutcome`] 等普通类型。

use super::{FailureReason, FailureStage, PrintError, PrintFailure, PrintOutcome};
use std::{fmt, time::Duration};
use tokio_util::sync::CancellationToken;
use tonic::{
    Code, Request,
    transport::{Channel, Endpoint},
};
use url::Url;

pub(super) mod proto {
    tonic::include_proto!("dzsrv");
}

const RPC_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(500);
const JOB_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const STATUS_RETRY_MIN: Duration = Duration::from_secs(1);
const STATUS_RETRY_MAX: Duration = Duration::from_secs(30);
const CANCEL_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) struct Client {
    print: proto::print_client::PrintClient<Channel>,
    mgmt: proto::mgmt_client::MgmtClient<Channel>,
}

impl Client {
    pub(super) fn connect(url: &Url) -> Result<Self, PrintError> {
        let endpoint = Endpoint::from_shared(url.as_str().to_owned())
            .map_err(|source| PrintError::Endpoint { source })?;
        let channel = endpoint.connect_lazy();
        Ok(Self {
            print: proto::print_client::PrintClient::new(channel.clone()),
            mgmt: proto::mgmt_client::MgmtClient::new(channel),
        })
    }

    pub(super) async fn submit(&mut self, tar: &[u8]) -> Result<JobId, PrintError> {
        let mut request = Request::new(proto::PrintTypstReq {
            typst_tar: tar.to_vec(),
            repeats: 0,
        });
        request.set_timeout(RPC_TIMEOUT);
        let handle = self
            .print
            .print_typst(request)
            .await
            .map_err(|source| PrintError::Submit { source })?
            .into_inner();
        JobId::from_bytes(&handle.job_id).ok_or(PrintError::BadJobId {
            len: handle.job_id.len(),
        })
    }

    pub(super) async fn wait(
        &mut self,
        job: JobId,
        ct: &CancellationToken,
    ) -> Result<PrintOutcome, PrintError> {
        let deadline = tokio::time::Instant::now() + JOB_TIMEOUT;
        let mut last_state: Option<proto::JobState> = None;
        let mut retry_delay = STATUS_RETRY_MIN;

        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err(PrintError::Timeout);
            }

            let status = match self.query(job, ct).await {
                Ok(status) => {
                    retry_delay = STATUS_RETRY_MIN;
                    status
                }
                Err(PrintError::Interrupted) => {
                    self.cancel_best_effort(job).await;
                    return Err(PrintError::Interrupted);
                }
                Err(err) if retryable_status(&err) => {
                    tracing::warn!(job = %job, "查询打印状态失败，稍后重试: {err}");
                    if super::sleep_or_cancel(retry_delay, ct).await.is_err() {
                        self.cancel_best_effort(job).await;
                        return Err(PrintError::Interrupted);
                    }
                    retry_delay = (retry_delay * 2).min(STATUS_RETRY_MAX);
                    continue;
                }
                Err(err) => return Err(err),
            };

            let state =
                proto::JobState::try_from(status.state).unwrap_or(proto::JobState::Unspecified);
            if last_state != Some(state) {
                tracing::info!(job = %job, state = state.as_str_name(), "打印状态");
                last_state = Some(state);
            }

            match state {
                proto::JobState::Succeeded => return Ok(PrintOutcome::Succeeded),
                proto::JobState::Cancelled => return Ok(PrintOutcome::Cancelled),
                proto::JobState::Failed => return Ok(PrintOutcome::Failed(job_failure(&status))),
                _ => {}
            }

            if super::sleep_or_cancel(POLL_INTERVAL, ct).await.is_err() {
                self.cancel_best_effort(job).await;
                return Err(PrintError::Interrupted);
            }
        }
    }

    async fn query(
        &mut self,
        job: JobId,
        ct: &CancellationToken,
    ) -> Result<proto::JobStatus, PrintError> {
        let mut request = Request::new(proto::JobHandle {
            job_id: job.as_bytes().to_vec(),
        });
        request.set_timeout(RPC_TIMEOUT);
        let result = tokio::select! {
            _ = ct.cancelled() => None,
            result = self.mgmt.status(request) => Some(result),
        };
        let Some(result) = result else {
            return Err(PrintError::Interrupted);
        };
        match result {
            Ok(reply) => Ok(reply.into_inner()),
            Err(source) if source.code() == Code::NotFound => Err(PrintError::JobNotFound),
            Err(source) => Err(PrintError::Status { source }),
        }
    }

    async fn cancel_best_effort(&mut self, job: JobId) {
        let mut request = Request::new(proto::JobHandle {
            job_id: job.as_bytes().to_vec(),
        });
        request.set_timeout(RPC_TIMEOUT);
        match tokio::time::timeout(CANCEL_TIMEOUT, self.mgmt.cancel(request)).await {
            Ok(Ok(reply)) => {
                if !reply.into_inner().accepted {
                    tracing::debug!(job = %job, "取消打印任务未被接受");
                }
            }
            Ok(Err(err)) => tracing::warn!(job = %job, "取消打印任务失败: {err}"),
            Err(_) => tracing::warn!(job = %job, "取消打印任务超时"),
        }
    }
}

fn retryable_status(err: &PrintError) -> bool {
    matches!(
        err,
        PrintError::Status { source }
            if matches!(
                source.code(),
                Code::Unavailable | Code::DeadlineExceeded | Code::Unknown | Code::Internal
            )
    )
}

fn job_failure(status: &proto::JobStatus) -> PrintFailure {
    let failure = status.failure.as_ref();
    PrintFailure {
        stage: failure
            .map(|failure| failure_stage(failure.stage))
            .unwrap_or(FailureStage::Unspecified),
        reason: failure
            .map(|failure| failure_reason(failure.reason))
            .unwrap_or(FailureReason::Unspecified),
        message: failure
            .map(|failure| failure.message.clone())
            .unwrap_or_default(),
        printer_status: printer_status(status.printer_status),
    }
}

fn failure_stage(value: i32) -> FailureStage {
    match proto::FailureStage::try_from(value) {
        Ok(proto::FailureStage::Render) => FailureStage::Render,
        Ok(proto::FailureStage::Connect) => FailureStage::Connect,
        Ok(proto::FailureStage::WaitForPrinter) => FailureStage::WaitForPrinter,
        Ok(proto::FailureStage::Print) => FailureStage::Print,
        _ => FailureStage::Unspecified,
    }
}

fn failure_reason(value: i32) -> FailureReason {
    match proto::FailureReason::try_from(value) {
        Ok(proto::FailureReason::Render) => FailureReason::Render,
        Ok(proto::FailureReason::DeviceNotFound) => FailureReason::DeviceNotFound,
        Ok(proto::FailureReason::DeviceNotReady) => FailureReason::DeviceNotReady,
        Ok(proto::FailureReason::PrinterStatus) => FailureReason::PrinterStatus,
        Ok(proto::FailureReason::PrinterSilent) => FailureReason::PrinterSilent,
        Ok(proto::FailureReason::PrintTimeout) => FailureReason::PrintTimeout,
        Ok(proto::FailureReason::PrintDidNotStart) => FailureReason::PrintDidNotStart,
        Ok(proto::FailureReason::Io) => FailureReason::Io,
        Ok(proto::FailureReason::Internal) => FailureReason::Internal,
        _ => FailureReason::Unspecified,
    }
}

fn printer_status(value: i32) -> Option<&'static str> {
    match proto::PrinterStatus::try_from(value) {
        Ok(proto::PrinterStatus::Unspecified) | Err(_) => None,
        Ok(status) => Some(status.as_str_name()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct JobId([u8; 16]);

impl JobId {
    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let bytes: [u8; 16] = bytes.try_into().ok()?;
        Some(Self(bytes))
    }

    fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("0x")?;
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_id_must_be_16_bytes() {
        assert!(JobId::from_bytes(&[0; 15]).is_none());
        assert!(JobId::from_bytes(&[0; 17]).is_none());
        assert!(JobId::from_bytes(&[0; 16]).is_some());
    }

    #[test]
    fn job_id_prints_hex() {
        let mut bytes = [0; 16];
        bytes[0] = 0xab;
        bytes[15] = 0x01;
        assert_eq!(
            JobId::from_bytes(&bytes).unwrap().to_string(),
            "0xab000000000000000000000000000001"
        );
    }

    #[test]
    fn maps_failure_stage_and_reason() {
        let status = proto::JobStatus {
            state: proto::JobState::Failed as i32,
            failure: Some(proto::JobFailure {
                stage: proto::FailureStage::Print as i32,
                reason: proto::FailureReason::PrinterSilent as i32,
                message: "打印机无响应".into(),
            }),
            printer_status: proto::PrinterStatus::PsCoverOpened as i32,
        };
        let failure = job_failure(&status);
        assert_eq!(failure.stage, FailureStage::Print);
        assert_eq!(failure.reason, FailureReason::PrinterSilent);
        assert_eq!(failure.message, "打印机无响应");
        assert_eq!(failure.printer_status, Some("PS_COVER_OPENED"));
    }

    #[test]
    fn missing_failure_is_unspecified() {
        let status = proto::JobStatus {
            state: proto::JobState::Failed as i32,
            failure: None,
            printer_status: proto::PrinterStatus::Unspecified as i32,
        };
        let failure = job_failure(&status);
        assert_eq!(failure.stage, FailureStage::Unspecified);
        assert_eq!(failure.reason, FailureReason::Unspecified);
        assert_eq!(failure.printer_status, None);
    }
}
