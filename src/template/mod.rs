//! 把模板源码渲染成可直接提交给 dzserver 的 tar。
//!
//! 这里不认识任何链，只接受“模板名 + 变量表”，方便以后接 SVM 等其它链。
//!
//! 占位符约定：模板里写成 `"{{key}}"`，必须独占一个 Typst 字符串字面量。
//! 渲染时整段（包括引号）按变量类型替换：
//!
//! - [`Value::Text`] 转义后放回字符串里；
//! - [`Value::Bool`] 替换成裸的 `true` / `false`；
//! - [`Value::Number`] 原样插入数字字面量。
//!
//! 严格模式：渲染完只要还有占位符就报错，不依赖模板里的演示值兜底。

mod assets;

use assets::{COMMON, IMAGES};
use snafu::{ResultExt, Snafu};
use std::io;

/// 模板变量。
#[derive(Debug, Clone)]
pub enum Value {
    /// 文本，会被转义并放进 Typst 字符串字面量
    Text(String),
    /// 原样插入的数字字面量
    Number(String),
    /// 原样插入的 `true` / `false`
    Bool(bool),
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

#[derive(Debug, Snafu)]
pub enum RenderError {
    #[snafu(display("未知模板: {name}"))]
    UnknownTemplate { name: String },
    #[snafu(display("模板 {name} 缺少变量: {keys}"))]
    MissingVariables { name: String, keys: String },
    #[snafu(display("模板 {name} 里有格式错误的占位符"))]
    BadPlaceholder { name: String },
    #[snafu(display("打包 tar 失败: {source}"))]
    Tar { source: io::Error },
}

/// 渲染指定模板，返回 tar 字节。
pub fn render_tar(name: &str, vars: &[(&str, Value)]) -> Result<Vec<u8>, RenderError> {
    let source = assets::source(name).ok_or_else(|| RenderError::UnknownTemplate {
        name: name.to_owned(),
    })?;
    let main = apply_vars(name, source, vars)?;
    build_tar(&main).context(TarSnafu)
}

/// 把源码里的占位符替换成变量值。
fn apply_vars(name: &str, source: &str, vars: &[(&str, Value)]) -> Result<String, RenderError> {
    let placeholders = scan_placeholders(name, source)?;

    let mut missing = Vec::new();
    for placeholder in &placeholders {
        if lookup(vars, placeholder.key).is_none() && !missing.contains(&placeholder.key) {
            missing.push(placeholder.key);
        }
    }
    if !missing.is_empty() {
        return Err(RenderError::MissingVariables {
            name: name.to_owned(),
            keys: missing.join(", "),
        });
    }

    let mut out = String::with_capacity(source.len() + 128);
    let mut cursor = 0;
    for placeholder in &placeholders {
        out.push_str(&source[cursor..placeholder.start]);
        // 上面已经确认每个占位符都有值
        let value = lookup(vars, placeholder.key).expect("占位符已检查");
        push_value(&mut out, value);
        cursor = placeholder.end;
    }
    out.push_str(&source[cursor..]);
    Ok(out)
}

struct Placeholder<'a> {
    key: &'a str,
    /// 整个 `"{{key}}"` 的起止位置
    start: usize,
    end: usize,
}

/// 扫出所有 `"{{key}}"`，并检查没有游离的 `{{`。
fn scan_placeholders<'a>(name: &str, source: &'a str) -> Result<Vec<Placeholder<'a>>, RenderError> {
    let bad = || RenderError::BadPlaceholder {
        name: name.to_owned(),
    };

    let mut placeholders = Vec::new();
    let mut cursor = 0;
    while let Some(offset) = source[cursor..].find("\"{{") {
        let start = cursor + offset;
        let key_start = start + 3; // 跳过引号和两个花括号
        let Some(key_len) = source[key_start..].find("}}\"") else {
            return Err(bad());
        };
        let key_end = key_start + key_len;
        let key = &source[key_start..key_end];
        if !valid_key(key) {
            return Err(bad());
        }
        placeholders.push(Placeholder {
            key,
            start,
            end: key_end + 3, // 两个花括号和收尾引号
        });
        cursor = key_end + 3;
    }

    // 合法的占位符都以 "{{ 开头，数量对不上说明有没写在字符串里的占位符
    if source.matches("{{").count() != placeholders.len() {
        return Err(bad());
    }
    Ok(placeholders)
}

fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn lookup<'a>(vars: &'a [(&str, Value)], key: &str) -> Option<&'a Value> {
    vars.iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value)
}

fn push_value(out: &mut String, value: &Value) {
    match value {
        Value::Text(text) => {
            out.push('"');
            push_escaped(out, text);
            out.push('"');
        }
        Value::Number(number) => out.push_str(number),
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
    }
}

/// 按 Typst 字符串字面量的规则转义。
fn push_escaped(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{{{:X}}}", ch as u32)),
            ch => out.push(ch),
        }
    }
}

/// 把 `main.typ`、`common.typ` 和图片按固定顺序打成 tar。
fn build_tar(main: &str) -> io::Result<Vec<u8>> {
    let mut builder = tar::Builder::new(Vec::new());
    append_file(&mut builder, "main.typ", main.as_bytes())?;
    append_file(&mut builder, "common.typ", COMMON.as_bytes())?;
    for (name, data) in IMAGES {
        append_file(&mut builder, name, data)?;
    }
    builder.into_inner()
}

fn append_file(builder: &mut tar::Builder<Vec<u8>>, name: &str, data: &[u8]) -> io::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_uid(0);
    header.set_gid(0);
    builder.append_data(&mut header, name, data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn text(value: &str) -> Value {
        Value::Text(value.to_owned())
    }

    #[test]
    fn replaces_text_with_escaping() {
        let source = r#"#let x = "{{name}}""#;
        let vars = [("name", text("a\"b\\c\nd"))];
        let out = apply_vars("t", source, &vars).unwrap();
        assert_eq!(out, r#"#let x = "a\"b\\c\nd""#);
    }

    #[test]
    fn replaces_bool_and_number_raw() {
        let source = r#"a = "{{flag}}", b = "{{count}}""#;
        let vars = [
            ("flag", Value::Bool(true)),
            ("count", Value::Number("42".into())),
        ];
        let out = apply_vars("t", source, &vars).unwrap();
        assert_eq!(out, r#"a = true, b = 42"#);
    }

    #[test]
    fn missing_variable_is_error() {
        let err = apply_vars("t", r#""{{name}}""#, &[]).unwrap_err();
        assert!(matches!(
            err,
            RenderError::MissingVariables { ref keys, .. } if keys == "name"
        ));
    }

    #[test]
    fn unquoted_placeholder_is_error() {
        let vars = [("name", text("x"))];
        let err = apply_vars("t", "{{name}}", &vars).unwrap_err();
        assert!(matches!(err, RenderError::BadPlaceholder { .. }));
    }

    #[test]
    fn unknown_template_is_error() {
        let err = render_tar("no-such-template", &[]).unwrap_err();
        assert!(matches!(err, RenderError::UnknownTemplate { .. }));
    }

    #[test]
    fn embedded_templates_render() {
        for (name, source) in assets::TEMPLATES {
            let placeholders = scan_placeholders(name, source).unwrap();
            let mut keys: Vec<&str> = placeholders.iter().map(|p| p.key).collect();
            keys.sort_unstable();
            keys.dedup();

            let vars: Vec<(&str, Value)> = keys
                .iter()
                .map(|key| {
                    if key.ends_with("_is_contract") {
                        (*key, Value::Bool(false))
                    } else {
                        (*key, text("x"))
                    }
                })
                .collect();

            let tar = render_tar(name, &vars).unwrap();
            let mut archive = tar::Archive::new(&tar[..]);
            let mut files = BTreeMap::new();
            for entry in archive.entries().unwrap() {
                let mut entry = entry.unwrap();
                let path = entry.path().unwrap().to_string_lossy().into_owned();
                let mut data = Vec::new();
                io::Read::read_to_end(&mut entry, &mut data).unwrap();
                files.insert(path, data);
            }

            assert!(files.contains_key("main.typ"), "{name} 缺少 main.typ");
            assert!(files.contains_key("common.typ"), "{name} 缺少 common.typ");
            assert_eq!(files.len(), 2 + IMAGES.len());
            let main = String::from_utf8(files.remove("main.typ").unwrap()).unwrap();
            assert!(!main.contains("{{"), "{name} 还有没替换的占位符");
        }
    }

    #[test]
    fn render_is_deterministic() {
        let first = render_tar("evm-erc20-recv", &complete_vars("evm-erc20-recv")).unwrap();
        let second = render_tar("evm-erc20-recv", &complete_vars("evm-erc20-recv")).unwrap();
        assert_eq!(first, second);
    }

    /// 给某个模板凑一份完整的占位符变量。
    fn complete_vars(name: &str) -> Vec<(&'static str, Value)> {
        let source = assets::source(name).unwrap();
        let placeholders = scan_placeholders(name, source).unwrap();
        let mut keys: Vec<&str> = placeholders.iter().map(|p| p.key).collect();
        keys.sort_unstable();
        keys.dedup();
        keys.into_iter()
            .map(|key| {
                if key.ends_with("_is_contract") {
                    (key, Value::Bool(false))
                } else {
                    (key, text("x"))
                }
            })
            .collect()
    }
}
