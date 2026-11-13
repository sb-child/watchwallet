#let tx(t: "", chunk-size: 32) = {
  let prefix = ""
  let body = t
  if t.starts-with("0x") {
    prefix = "       0x" // 和上面的"xxxx:"拼到一起
    body = t.slice(2)
  }
  let chunks = ()
  let len = body.len()
  let i = 0
  while i < len {
    let end = calc.min(i + chunk-size, len)
    chunks.push(body.slice(i, end))
    i += chunk-size
  }
  let lines = if prefix != "" { (prefix, ..chunks) } else { chunks }
  align(center)[
    #block()[
      #set align(left)
      #text(font: "JetBrains Mono", size: 7pt, lines.join([
        #v(-6pt)
      ]))
    ]
  ]
}

#let account_with_name(name: "", addr: "", contract: false) = {
  let im = if contract {
    "receipt-text.svg"
  } else {
    "user-round-star.svg"
  }
  rect(
    radius: 5pt,
    stroke: 0.5pt + black,
    inset: 1pt,
    grid(
      columns: (14pt, 1fr),
      gutter: 2pt,
      align: left,
      [#image(im, width: 14pt)],
      [
        #align(horizon + left)[
          #text(name, size: 10pt)
          #v(-10pt)
          #text(addr, font: "JetBrains Mono", size: 4.6pt, weight: 800)
        ]
      ],
    ),
  )
}

#let account_only_addr(addr: "", contract: false) = {
  let im = if contract {
    "receipt-text.svg"
  } else {
    "user-round.svg"
  }
  rect(
    radius: 5pt,
    stroke: 0.5pt + black,
    inset: 1pt,
    grid(
      columns: (14pt, 1fr),
      gutter: 2pt,
      align: left,
      [#image(im, width: 14pt)],
      [
        #align(horizon + left)[
          #text(addr, font: "JetBrains Mono", size: 4.6pt, weight: 800)
        ]
      ],
    ),
  )
}

#let account(name: "", addr: "", contract: false, note: "") = {
  pad(left: 0.6pt, top: 0pt)[
    #if name == "" {
      account_only_addr(addr: addr, contract: contract)
    } else {
      account_with_name(name: name, addr: addr, contract: contract)
    }
    #if note != "" {
      place(
        top + right,
        dy: -4pt,
        dx: 1pt,
        block(fill: rgb("#ffffffff"), inset: 1pt)[
          #text(note, weight: "bold", size: 4.8pt)
        ],
      )
    }

  ]
}


#let flow-down(label: "") = {
  grid(
    columns: (1fr, 5fr),
    gutter: 2pt,
    align: center,
    [#image("chevrons-down.svg", width: 12pt)],
    [
      #align(horizon + left)[
        #text(label, size: 10pt)
      ]
    ],
  )
}

#let flow-up(label: "") = {
  grid(
    columns: (1fr, 5fr),
    gutter: 2pt,
    align: center,
    [#image("chevrons-up.svg", width: 12pt)],
    [
      #align(horizon + left)[
        #text(label, size: 10pt)
      ]
    ],
  )
}

#let fit-text(
  body,
  max-size: 12pt,
  min-size: 6pt,
  step: 0.2pt,
  weight: "regular",
) = {
  layout(container-size => context {
    let max-width = container-size.width
    let current-size = max-size
    let text-width = measure(text(size: current-size, top-edge: "bounds", weight: weight)[#body]).width
    while text-width > max-width and current-size > min-size {
      current-size -= step
      text-width = measure(text(size: current-size, top-edge: "bounds", weight: weight)[#body]).width
    }
    text(size: current-size, weight: weight)[#body]
  })
}

#let title(t, revoked: false) = {
  let _t(im, t: "") = {
    [= #box(image(im, width: 14pt), baseline: 3pt) #text(t)]
  }
  if t == "approve" and revoked {
    _t("feather.svg", t: "取消授权")
  } else if t == "approve" {
    _t("feather.svg", t: "授权代币")
  } else if t == "approve-recv" and revoked {
    _t("feather.svg", t: "授权被取消")
  } else if t == "approve-recv" {
    _t("feather.svg", t: "收到授权")
  } else if t == "erc20-recv" {
    _t("hand-coins.svg", t: "收到代币")
  } else if t == "erc20-send" {
    _t("send.svg", t: "转出代币")
  } else if t == "recv" {
    _t("hand-coins.svg", t: "收到原生币")
  } else if t == "send" {
    _t("send.svg", t: "转出原生币")
  }
}

#let float_to_str(f) = {
  let s = if type(f) == str {
    f
  } else {
    str(calc.round(float(f), digits: 18))
  }
  return s.replace(regex("\.0+$|(\.\d+?)0+$"), "$1")
}

#let display-value(val: 0.0, sym: "", name: "", k: "") = {
  let prefix = if k == "inc" {
    "+"
  } else if k == "dec" {
    "-"
  } else {
    ""
  }
  let value = if val == "inf" { "无限制" } else { float_to_str(val) }
  let value_str = prefix + value + if sym == "" { "" } else { " " + sym }
  align(center, fit-text(value_str, max-size: 12pt))
  if name != "" {
    v(-4pt)
    align(center, text(name, size: 6pt, weight: "bold"))
  }
  v(-4pt)
}

#let details(
  _datetime: "",
  _chain_name: "",
  _tx_fee: 0.0,
  _sym: "",
  _token_contract: "",
  _txid: "",
) = {
  let value_str = float_to_str(_tx_fee) + " " + _sym
  [
    - 时间: #text(_datetime, weight: "bold")
    - 区块链: #text(_chain_name, weight: "bold")
    // 交易费未知时传空串，整行隐藏
    #if _tx_fee != "" [
      #v(-4.5pt)
      #grid(
        columns: (auto, auto),
        align: left,
        gutter: 2pt,
        [- 网络费:],
        [
          #align(horizon)[
            #fit-text(value_str, weight: "bold", max-size: 8pt)
          ]
        ],
      )
    ]
    #v(-4.5pt)
    #if _token_contract != "" {
      [
        - 代币:
        #v(-15pt) // 把"代币:"与"0x"叠在一起
        #tx(t: _token_contract)
        #v(-5pt)
      ]
    }
    #if _txid != "" {
      [
        - 交易:
        #v(-15pt) // 把"交易:"与"0x"叠在一起
        #tx(t: _txid)
        #v(-5pt)
      ]
    }
  ]
}

#let temp(x: "{{}}", def: "") = {
  // 渲染后布尔值/数字会替换成裸字面量，只有字符串才需要检查占位符
  if type(x) == str and x.contains("{{") and x.contains("}}") {
    def
  } else {
    x
  }
}
