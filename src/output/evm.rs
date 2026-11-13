//! EVM 事件到打印模板的映射。
//!
//! `crate::template` 只认模板名和变量表，这里负责选模板、把链上数据格式化成模板变量。

use crate::{
    config::EvmConfig,
    evm::event::{
        AccountInfo, Erc20TokenInfo, EvmEvent, NativeTransferEvent, TokenApprovalEvent,
        TokenTransferEvent, TransferFlow, TxInfo,
    },
    template::{self, RenderError, Value},
};
use alloy::primitives::{B256, Keccak256, U256, hex};
use chrono::{DateTime, Local, Utc};

/// 原生币的小数位，EVM 链统一是 18。
const NATIVE_DECIMALS: u8 = 18;

/// 渲染好的打印包。
#[derive(Debug)]
pub struct Rendered {
    /// 使用的模板名
    pub template: &'static str,
    /// 可直接提交给 dzserver 的 tar
    pub tar: Vec<u8>,
}

/// 把一条 EVM 事件渲染成打印包。
pub fn render(event: &EvmEvent, cfg: &EvmConfig) -> Result<Rendered, RenderError> {
    let name = template_name(event);
    let vars = build_vars(event, cfg);
    let tar = template::render_tar(name, &vars)?;
    Ok(Rendered {
        template: name,
        tar,
    })
}

fn template_name(event: &EvmEvent) -> &'static str {
    match event {
        EvmEvent::TokenTransfer(event) if event.is_send() => "evm-erc20-send",
        EvmEvent::TokenTransfer(_) => "evm-erc20-recv",
        EvmEvent::TokenApproval(event) if event.watched_is_owner() => "evm-erc20-approve",
        EvmEvent::TokenApproval(_) => "evm-erc20-approve-recv",
        EvmEvent::NativeTransfer(event) if event.is_send() => "evm-native-send",
        EvmEvent::NativeTransfer(_) => "evm-native-recv",
    }
}

// 约定：拿不到的值一律传空字符串（没名字、没手续费、没 txid 等），由模板决定要不要隐藏。
fn build_vars(event: &EvmEvent, cfg: &EvmConfig) -> Vec<(&'static str, Value)> {
    match event {
        EvmEvent::TokenTransfer(event) => transfer_vars(event, cfg),
        EvmEvent::TokenApproval(event) => approval_vars(event, cfg),
        EvmEvent::NativeTransfer(event) => native_vars(event, cfg),
    }
}

fn transfer_vars(event: &TokenTransferEvent, cfg: &EvmConfig) -> Vec<(&'static str, Value)> {
    let mut vars = base_vars(
        cfg,
        event.tx_hash,
        event.tx_info.as_ref(),
        format_amount(event.amount, event.token.decimals),
    );
    push_token(&mut vars, &event.token);
    push_account(
        &mut vars,
        "sender_addr",
        "sender_name",
        "sender_is_contract",
        event.sender(),
    );
    push_account(
        &mut vars,
        "recp_addr",
        "recp_name",
        "recp_is_contract",
        event.recipient(),
    );
    vars
}

fn approval_vars(event: &TokenApprovalEvent, cfg: &EvmConfig) -> Vec<(&'static str, Value)> {
    let mut vars = base_vars(
        cfg,
        event.tx_hash,
        event.tx_info.as_ref(),
        approval_amount(event.amount, event.token.decimals),
    );
    push_token(&mut vars, &event.token);
    push_account(
        &mut vars,
        "owner_addr",
        "owner_name",
        "owner_is_contract",
        &event.owner,
    );
    push_account(
        &mut vars,
        "spender_addr",
        "spender_name",
        "spender_is_contract",
        &event.spender,
    );
    vars
}

fn native_vars(event: &NativeTransferEvent, cfg: &EvmConfig) -> Vec<(&'static str, Value)> {
    let mut vars = base_vars(
        cfg,
        event.tx_hash,
        event.tx_info.as_ref(),
        format_amount(event.amount, NATIVE_DECIMALS),
    );
    vars.push(("coin_symbol", Value::from(&*cfg.coin)));
    vars.push(("coin_name", Value::from("")));
    push_account(
        &mut vars,
        "sender_addr",
        "sender_name",
        "sender_is_contract",
        event.sender(),
    );
    push_account(
        &mut vars,
        "recp_addr",
        "recp_name",
        "recp_is_contract",
        event.recipient(),
    );
    vars
}

/// 所有模板共用的变量。
fn base_vars(
    cfg: &EvmConfig,
    tx_hash: B256,
    tx_info: Option<&TxInfo>,
    transfer_value: String,
) -> Vec<(&'static str, Value)> {
    vec![
        ("chain_name", Value::from(&*cfg.chain_name)),
        ("datetime", Value::from(datetime_text(tx_info))),
        ("tx_fee", Value::from(tx_fee_text(tx_info))),
        ("tx_fee_coin_symbol", Value::from(&*cfg.coin)),
        ("tx_id", Value::from(checksum_hex(tx_hash.as_slice()))),
        ("transfer_value", Value::from(transfer_value)),
    ]
}

fn push_token(vars: &mut Vec<(&'static str, Value)>, token: &Erc20TokenInfo) {
    vars.push((
        "coin_symbol",
        Value::from(token.symbol.as_deref().unwrap_or("")),
    ));
    vars.push((
        "coin_name",
        Value::from(token.name.as_deref().unwrap_or("")),
    ));
    vars.push((
        "token_contract",
        Value::from(checksum_hex(token.contract.as_slice())),
    ));
}

fn push_account(
    vars: &mut Vec<(&'static str, Value)>,
    address_key: &'static str,
    name_key: &'static str,
    contract_key: &'static str,
    account: &AccountInfo,
) {
    vars.push((
        address_key,
        Value::from(checksum_hex(account.addr.as_slice())),
    ));
    vars.push((name_key, Value::from(account.name.as_deref().unwrap_or(""))));
    vars.push((contract_key, Value::Bool(account.is_contract)));
}

// 约定：模板里的所有 0x 值都显示成混合大小写（EIP-55 风格）。
// EIP-55 只定义在 20 字节地址上，哈希也套用同一算法，保证观感一致；十六进制大小写不影响复制粘贴。
/// 按 EIP-55 生成混合大小写的十六进制字符串。
fn checksum_hex(bytes: &[u8]) -> String {
    let lower = hex::encode(bytes);
    let mut hasher = Keccak256::new();
    hasher.update(lower.as_bytes());
    let hash = hasher.finalize();

    let mut out = String::with_capacity(2 + lower.len());
    out.push_str("0x");
    for (index, ch) in lower.chars().enumerate() {
        let nibble = (hash[index / 2] >> if index % 2 == 0 { 4 } else { 0 }) & 0x0f;
        if ch.is_ascii_alphabetic() && nibble >= 8 {
            out.push(ch.to_ascii_uppercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// 链上整数按 decimals 转成十进制字符串，去掉多余的零。
fn format_amount(amount: U256, decimals: u8) -> String {
    let text = alloy::primitives::utils::format_units(amount, decimals)
        .unwrap_or_else(|_| amount.to_string());
    trim_trailing_zeros(text)
}

fn trim_trailing_zeros(mut text: String) -> String {
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    text
}

/// 授权额度，`type(uint256).max` 用模板认识的无限制标记 `inf` 表示。
fn approval_amount(amount: U256, decimals: u8) -> String {
    if amount == U256::MAX {
        "inf".to_owned()
    } else {
        format_amount(amount, decimals)
    }
}

/// 出块时间；拿不到交易信息时用本地收到通知的时间。
fn datetime_text(tx_info: Option<&TxInfo>) -> String {
    let timestamp = tx_info
        .map(|info| info.timestamp)
        .unwrap_or_else(now_timestamp);
    DateTime::<Utc>::from_timestamp(timestamp as i64, 0)
        .map(|time| {
            time.with_timezone(&Local)
                .format("%Y/%m/%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}

fn now_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// 交易费；拿不到交易信息时留空，模板会隐藏这一行。
fn tx_fee_text(tx_info: Option<&TxInfo>) -> String {
    tx_info
        .map(|info| format_amount(info.fee, NATIVE_DECIMALS))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evm::event::TransferDirection;
    use alloy::primitives::{Address, U256, address, b256};

    const SENDER: Address = address!("0x233b7f746aBAc0797E8dBC344b638F1C4225F264");
    const RECP: Address = address!("0x5838c07a50894fAA58EBfe5E5975f1FCc8792cD3");
    const TOKEN: Address = address!("0xcac524bca292aaade2df8a05cc58f0a65b1b3bb9");
    const TX: B256 = b256!("0xce656824ee4da940ca08901088940d52b4fe1b5a7b4b21007daaec4f1c315529");

    fn cfg() -> EvmConfig {
        EvmConfig {
            chain_name: "Arbitrum One".into(),
            coin: "ETH".into(),
            ws_url: "ws://localhost:8546".parse().unwrap(),
            native_transfer: true,
            address: Vec::new().into_boxed_slice(),
        }
    }

    fn tx_info() -> TxInfo {
        TxInfo {
            fee: U256::from(24_000_000_000_000u64),
            timestamp: 1_794_492_996,
            success: true,
        }
    }

    fn account(addr: Address, name: Option<&str>, is_contract: bool) -> AccountInfo {
        AccountInfo {
            addr,
            name: name.map(Into::into),
            is_contract,
        }
    }

    fn token() -> Erc20TokenInfo {
        Erc20TokenInfo {
            contract: TOKEN,
            symbol: Some("PYUSD".into()),
            name: Some("PayPal USD".into()),
            decimals: 6,
        }
    }

    fn token_transfer(direction: TransferDirection, tx_info: Option<TxInfo>) -> EvmEvent {
        EvmEvent::TokenTransfer(Box::new(TokenTransferEvent {
            watched: account(RECP, Some("test wallet"), false),
            counterparty: account(SENDER, None, false),
            direction,
            token: token(),
            amount: U256::from(123_456_789u64),
            tx_hash: TX,
            tx_info,
        }))
    }

    fn native_transfer(direction: TransferDirection, tx_info: Option<TxInfo>) -> EvmEvent {
        EvmEvent::NativeTransfer(Box::new(NativeTransferEvent {
            watched: account(RECP, Some("test wallet"), false),
            counterparty: account(SENDER, None, false),
            direction,
            amount: U256::from(50_000_000_000_000_000u64),
            tx_hash: TX,
            tx_info,
        }))
    }

    fn main_typ(tar: &[u8]) -> String {
        let mut archive = tar::Archive::new(tar);
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            if entry.path().unwrap().to_string_lossy() == "main.typ" {
                let mut data = String::new();
                std::io::Read::read_to_string(&mut entry, &mut data).unwrap();
                return data;
            }
        }
        panic!("tar 里没有 main.typ");
    }

    #[test]
    fn checksum_matches_eip55_vectors() {
        let vectors = [
            (
                "0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed",
                "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
            ),
            (
                "0xfb6916095ca1df60bb79ce92ce3ea74c37c5d359",
                "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359",
            ),
            (
                "0xdbf03b407c01e7cd3cbea99509d93f8dddc8c6fb",
                "0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB",
            ),
            (
                "0xd1220a0cf47c7b9be7a2e6ba89f429762e7b9adb",
                "0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb",
            ),
        ];
        for (input, expected) in vectors {
            let address: Address = input.parse().unwrap();
            assert_eq!(checksum_hex(address.as_slice()), expected);
        }
    }

    #[test]
    fn hash_keeps_hex_value() {
        let text = checksum_hex(TX.as_slice());
        assert_eq!(
            text.to_ascii_lowercase(),
            format!("0x{}", hex::encode(TX.as_slice()))
        );
        assert_ne!(text, text.to_ascii_lowercase());
    }

    #[test]
    fn renders_erc20_send() {
        let event = token_transfer(TransferDirection::Send, Some(tx_info()));
        let out = render(&event, &cfg()).unwrap();
        assert_eq!(out.template, "evm-erc20-send");

        let main = main_typ(&out.tar);
        assert!(main.contains(r#"x: "123.456789", def:"#));
        assert!(main.contains(r#"x: "PYUSD", def:"#));
        assert!(main.contains(r#"x: "PayPal USD", def:"#));
        assert!(main.contains(r#"x: "Arbitrum One", def:"#));
        assert!(main.contains(r#"x: "0x5838c07a50894fAA58EBfe5E5975f1FCc8792cD3", def:"#));
        assert!(main.contains(r#"x: "0x233b7f746aBAc0797E8dBC344b638F1C4225F264", def:"#));
        assert!(main.contains(r#"x: "0.000024", def:"#));
        assert!(!main.contains("{{"));
    }

    #[test]
    fn renders_erc20_recv_without_tx_info() {
        let event = token_transfer(TransferDirection::Recv, None);
        let out = render(&event, &cfg()).unwrap();
        assert_eq!(out.template, "evm-erc20-recv");

        let main = main_typ(&out.tar);
        // 拿不到手续费时传空串，模板会隐藏网络费
        assert!(main.contains(r#"x: "", def: "0.3333333332222333""#));
    }

    #[test]
    fn missing_values_are_empty_strings() {
        let event = EvmEvent::TokenTransfer(Box::new(TokenTransferEvent {
            watched: account(RECP, None, false),
            counterparty: account(SENDER, None, false),
            direction: TransferDirection::Recv,
            token: Erc20TokenInfo {
                contract: TOKEN,
                symbol: None,
                name: None,
                decimals: 6,
            },
            amount: U256::from(1_000_000u64),
            tx_hash: TX,
            tx_info: None,
        }));
        let out = render(&event, &cfg()).unwrap();
        let main = main_typ(&out.tar);

        // 没有 symbol / name / 手续费 / 地址名字时都是空字符串
        assert!(main.contains(r#"x: "", def: "USDT""#));
        assert!(main.contains(r#"x: "", def: "Tether USD""#));
        assert!(main.contains(r#"x: "", def: "0.3333333332222333""#));
        assert!(main.contains(r#"x: "", def: "sbchild钱包1""#));
        // tx_id 一定有值，且显示成混合大小写
        assert!(main.contains(
            r#"x: "0xCE656824EE4dA940ca08901088940D52b4Fe1B5a7b4b21007dAAEc4F1c315529", def:"#
        ));
    }

    #[test]
    fn native_coin_name_is_always_empty() {
        let out = render(&native_transfer(TransferDirection::Recv, None), &cfg()).unwrap();
        let main = main_typ(&out.tar);
        assert!(main.contains(r#"x: "", def: """#));
    }

    fn approval(direction: TransferDirection, amount: U256) -> EvmEvent {
        EvmEvent::TokenApproval(Box::new(TokenApprovalEvent {
            owner: account(RECP, Some("test wallet"), false),
            spender: account(SENDER, None, true),
            direction,
            token: token(),
            amount,
            tx_hash: TX,
            tx_info: Some(tx_info()),
        }))
    }

    #[test]
    fn approval_uses_inf_for_max_amount() {
        let out = render(&approval(TransferDirection::Send, U256::MAX), &cfg()).unwrap();
        assert_eq!(out.template, "evm-erc20-approve");

        let main = main_typ(&out.tar);
        assert!(main.contains(r#"x: "inf""#));
        assert!(main.contains(r#"x: true"#));
    }

    #[test]
    fn received_approval_uses_recv_template() {
        let out = render(
            &approval(TransferDirection::Recv, U256::from(1_000_000u64)),
            &cfg(),
        )
        .unwrap();
        assert_eq!(out.template, "evm-erc20-approve-recv");

        let main = main_typ(&out.tar);
        // 被授权的一方排在前面，箭头指向上方
        assert!(main.contains(r#"common.title("approve-recv""#));
        assert!(main.contains("授权来自"));
        assert!(main.contains(r#"x: "1""#));
    }

    #[test]
    fn zero_approval_is_marked_as_revoke() {
        let out = render(&approval(TransferDirection::Send, U256::ZERO), &cfg()).unwrap();
        assert_eq!(out.template, "evm-erc20-approve");

        let main = main_typ(&out.tar);
        // 是否取消授权由模板根据数量判断
        assert!(main.contains(r#"revoked: _transfer_value == "0""#));
    }

    #[test]
    fn renders_native_send() {
        let event = native_transfer(TransferDirection::Send, Some(tx_info()));
        let out = render(&event, &cfg()).unwrap();
        assert_eq!(out.template, "evm-native-send");

        let main = main_typ(&out.tar);
        assert!(main.contains(r#"x: "0.05""#));
        assert!(main.contains(r#"x: "ETH""#));
        assert!(!main.contains("token_contract"));
    }

    #[test]
    #[ignore = "需要本机安装 typst"]
    fn rendered_templates_compile() {
        let events = [
            token_transfer(TransferDirection::Send, Some(tx_info())),
            token_transfer(TransferDirection::Recv, None),
            approval(TransferDirection::Send, U256::MAX),
            approval(TransferDirection::Recv, U256::from(1_000_000u64)),
            approval(TransferDirection::Send, U256::ZERO),
            approval(TransferDirection::Recv, U256::ZERO),
            native_transfer(TransferDirection::Send, Some(tx_info())),
            native_transfer(TransferDirection::Recv, None),
        ];

        for event in &events {
            let rendered = render(event, &cfg()).unwrap();
            let dir = tempfile::tempdir().unwrap();
            tar::Archive::new(&rendered.tar[..])
                .unpack(dir.path())
                .unwrap();
            let status = std::process::Command::new("typst")
                .arg("compile")
                .arg(dir.path().join("main.typ"))
                .arg(dir.path().join("out.pdf"))
                .status()
                .expect("运行 typst 失败");
            assert!(status.success(), "模板 {} 编译失败", rendered.template);
        }
    }
}
