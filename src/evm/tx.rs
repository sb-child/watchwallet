use super::EvmProvider;
use super::event::TxInfo;
use alloy::{
    primitives::{B256, U256},
    providers::Provider as _,
};

/// 查询交易手续费和出块时间。
///
/// - receipt 还没被节点索引到：返回 [`None`]，不打印日志，也不写缓存。
/// - provider 出错：打印日志后返回 [`None`]，不写缓存，等下次事件再试。
pub async fn fetch_tx_info(p: &EvmProvider, txid: B256) -> Option<TxInfo> {
    fetch_tx_info_inner(p, txid, None).await
}

/// 已经知道出块时间时用这个，省掉一次区块查询。原生币转账在扫区块时能直接拿到时间戳。
pub async fn fetch_tx_info_at(p: &EvmProvider, txid: B256, timestamp: u64) -> Option<TxInfo> {
    fetch_tx_info_inner(p, txid, Some(timestamp)).await
}

async fn fetch_tx_info_inner(
    p: &EvmProvider,
    txid: B256,
    timestamp: Option<u64>,
) -> Option<TxInfo> {
    let receipt = match p.get_transaction_receipt(txid).await {
        Ok(Some(receipt)) => receipt,
        Ok(None) => return None,
        Err(err) => {
            tracing::error!(tx_hash = %txid, "查询交易回执失败: {err}");
            return None;
        }
    };
    let timestamp = match timestamp {
        Some(timestamp) => timestamp,
        None => {
            let block_hash = receipt.block_hash?;
            let block = match p.get_block_by_hash(block_hash).await {
                Ok(Some(block)) => block,
                Ok(None) => return None,
                Err(err) => {
                    tracing::error!(tx_hash = %txid, %block_hash, "查询区块失败: {err}");
                    return None;
                }
            };
            block.header.timestamp
        }
    };
    Some(TxInfo {
        // gas_used * effective_gas_price 已经包含 L2 的 L1 费用部分
        fee: U256::from(receipt.gas_used) * U256::from(receipt.effective_gas_price),
        timestamp,
        success: receipt.inner.status(),
    })
}
