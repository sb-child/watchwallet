use alloy::primitives::{Address, B256, U256};

/// 方向，始终以被监听地址为参照。转账和授权共用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferDirection {
    /// 被监听地址失去代币，或把代币授权给别人：watched -> counterparty
    Send,
    /// 被监听地址收到代币，或被别人授权：counterparty -> watched
    Recv,
}

/// 按方向决定哪一边是被监听地址，返回 `(watched, counterparty)`。
pub(crate) fn sides(
    direction: TransferDirection,
    from: Address,
    to: Address,
) -> (Address, Address) {
    match direction {
        TransferDirection::Send => (from, to),
        TransferDirection::Recv => (to, from),
    }
}

/// 地址及其在配置里的名字。
#[derive(Debug, Clone)]
pub struct AccountInfo {
    pub addr: Address,
    /// 配置里为该地址写的名字
    pub name: Option<Box<str>>,
    pub is_contract: bool,
}

/// ERC-20 代币信息。
#[derive(Debug, Clone)]
pub struct Erc20TokenInfo {
    pub contract: Address,
    pub symbol: Option<Box<str>>,
    pub name: Option<Box<str>>,
    pub decimals: u8,
}

/// 交易手续费、出块时间与执行状态。
#[derive(Debug, Clone)]
pub struct TxInfo {
    /// 手续费，单位是链的原生币最小单位
    pub fee: U256,
    /// 出块时间，Unix 秒
    pub timestamp: u64,
    /// 交易是否执行成功
    // 失败的交易不会真正转账，但链上仍然记录了 value。
    pub success: bool,
}

/// 以被监听地址为参照的转账事件：`watched` 始终是被监听的一方。
pub trait TransferFlow {
    fn watched(&self) -> &AccountInfo;
    fn counterparty(&self) -> &AccountInfo;
    fn direction(&self) -> TransferDirection;

    /// 链上失去币的一方
    fn sender(&self) -> &AccountInfo {
        match self.direction() {
            TransferDirection::Send => self.watched(),
            TransferDirection::Recv => self.counterparty(),
        }
    }

    /// 链上获得币的一方
    fn recipient(&self) -> &AccountInfo {
        match self.direction() {
            TransferDirection::Send => self.counterparty(),
            TransferDirection::Recv => self.watched(),
        }
    }

    fn is_send(&self) -> bool {
        self.direction() == TransferDirection::Send
    }

    fn is_recv(&self) -> bool {
        self.direction() == TransferDirection::Recv
    }
}

// 约定：watched 始终是被监听的一方，direction 说明币的流向。
/// ERC-20 Transfer 事件，同时表示收款和转账。
#[derive(Debug, Clone)]
pub struct TokenTransferEvent {
    /// 本次通知对应的被监听地址
    pub watched: AccountInfo,
    /// 交易对手方
    pub counterparty: AccountInfo,
    /// 方向
    pub direction: TransferDirection,
    /// 交易代币
    pub token: Erc20TokenInfo,
    /// 链上原始数量
    pub amount: U256,
    pub tx_hash: B256,
    /// 查询失败时为 None，模板使用默认值
    pub tx_info: Option<TxInfo>,
}

impl TransferFlow for TokenTransferEvent {
    fn watched(&self) -> &AccountInfo {
        &self.watched
    }

    fn counterparty(&self) -> &AccountInfo {
        &self.counterparty
    }

    fn direction(&self) -> TransferDirection {
        self.direction
    }
}

/// 原生币转账事件。
#[derive(Debug, Clone)]
pub struct NativeTransferEvent {
    /// 本次通知对应的被监听地址
    pub watched: AccountInfo,
    /// 交易对手方
    pub counterparty: AccountInfo,
    /// 方向
    pub direction: TransferDirection,
    /// 链上原始数量
    pub amount: U256,
    pub tx_hash: B256,
    /// 查询失败时为 None，模板使用默认值
    pub tx_info: Option<TxInfo>,
}

impl TransferFlow for NativeTransferEvent {
    fn watched(&self) -> &AccountInfo {
        &self.watched
    }

    fn counterparty(&self) -> &AccountInfo {
        &self.counterparty
    }

    fn direction(&self) -> TransferDirection {
        self.direction
    }
}

/// ERC-20 Approval 事件。
#[derive(Debug, Clone)]
pub struct TokenApprovalEvent {
    /// 持有代币、做出授权的一方
    pub owner: AccountInfo,
    /// 被授权的账户
    pub spender: AccountInfo,
    /// 方向：`Send` 表示被监听地址是 owner，`Recv` 表示是 spender
    pub direction: TransferDirection,
    /// 授权代币
    pub token: Erc20TokenInfo,
    /// 授权数量。可以是无限。
    pub amount: U256,
    pub tx_hash: B256,
    /// 查询失败时为 None，模板使用默认值
    pub tx_info: Option<TxInfo>,
}

impl TokenApprovalEvent {
    /// 被监听地址是否是做出授权的一方。
    pub fn watched_is_owner(&self) -> bool {
        self.direction == TransferDirection::Send
    }
}

#[derive(Debug, Clone)]
pub enum EvmEvent {
    TokenTransfer(Box<TokenTransferEvent>),
    TokenApproval(Box<TokenApprovalEvent>),
    NativeTransfer(Box<NativeTransferEvent>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::address;

    #[test]
    fn sides_follow_direction() {
        let from = address!("0x5838c07a50894fAA58EBfe5E5975f1FCc8792cD3");
        let to = address!("0x233b7f746aBAc0797E8dBC344b638F1C4225F264");
        assert_eq!(sides(TransferDirection::Send, from, to), (from, to));
        assert_eq!(sides(TransferDirection::Recv, from, to), (to, from));
    }
}
