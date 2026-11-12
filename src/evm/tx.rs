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
    let receipt = match p.get_transaction_receipt(txid).await {
        Ok(Some(receipt)) => receipt,
        Ok(None) => return None,
        Err(err) => {
            tracing::error!(tx_hash = %txid, "查询交易回执失败: {err}");
            return None;
        }
    };
    let Some(block_hash) = receipt.block_hash else {
        return None;
    };
    let block = match p.get_block_by_hash(block_hash).await {
        Ok(Some(block)) => block,
        Ok(None) => return None,
        Err(err) => {
            tracing::error!(tx_hash = %txid, %block_hash, "查询区块失败: {err}");
            return None;
        }
    };
    Some(TxInfo {
        // gas_used * effective_gas_price 已经包含 L2 的 L1 费用部分
        fee: U256::from(receipt.gas_used) * U256::from(receipt.effective_gas_price),
        timestamp: block.header.timestamp,
    })
}
