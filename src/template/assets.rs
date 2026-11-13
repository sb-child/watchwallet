//! 编译期嵌入的 Typst 模板资源。
//!
//! 加模板时在 [`TEMPLATES`] 里加一行；图片统一打进 tar，改动很少。

/// 所有模板共用的函数。
pub(crate) const COMMON: &str = include_str!("../../template/common.typ");

/// 模板文件名（不含 `.typ`）到源码的映射。
pub(crate) const TEMPLATES: &[(&str, &str)] = &[
    (
        "evm-erc20-recv",
        include_str!("../../template/evm-erc20-recv.typ"),
    ),
    (
        "evm-erc20-send",
        include_str!("../../template/evm-erc20-send.typ"),
    ),
    (
        "evm-erc20-approve",
        include_str!("../../template/evm-erc20-approve.typ"),
    ),
    (
        "evm-erc20-approve-recv",
        include_str!("../../template/evm-erc20-approve-recv.typ"),
    ),
    (
        "evm-native-recv",
        include_str!("../../template/evm-native-recv.typ"),
    ),
    (
        "evm-native-send",
        include_str!("../../template/evm-native-send.typ"),
    ),
];

/// 模板引用的图片，统一放进 tar 根目录。
pub(crate) const IMAGES: &[(&str, &[u8])] = &[
    (
        "chevrons-down.svg",
        include_bytes!("../../template/chevrons-down.svg"),
    ),
    (
        "chevrons-up.svg",
        include_bytes!("../../template/chevrons-up.svg"),
    ),
    ("feather.svg", include_bytes!("../../template/feather.svg")),
    (
        "hand-coins.svg",
        include_bytes!("../../template/hand-coins.svg"),
    ),
    (
        "receipt-text.svg",
        include_bytes!("../../template/receipt-text.svg"),
    ),
    ("send.svg", include_bytes!("../../template/send.svg")),
    (
        "user-round-star.svg",
        include_bytes!("../../template/user-round-star.svg"),
    ),
    (
        "user-round.svg",
        include_bytes!("../../template/user-round.svg"),
    ),
];

pub(crate) fn source(name: &str) -> Option<&'static str> {
    TEMPLATES
        .iter()
        .find(|(template, _)| *template == name)
        .map(|(_, source)| *source)
}
