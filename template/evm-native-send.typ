#set page(width: 48mm, height: auto, margin: (top: 1mm, bottom: 1mm, left: 0.2mm, right: 0mm))
#set text(font: "LXGW WenKai GB", size: 8pt)
#let wrap(x) = {
  x.codepoints().join(sym.zws)
}

#let tx(t: "", chunk-size: 32) = {
  let prefix = ""
  let body = t
  if t.starts-with("0x") {
    prefix = "      0x" // 和上面的"交易:"拼到一起
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
      #text(font: "Unifont", lines.join([
        #v(-8pt)
      ]))
    ]

  ]
}

#let account_with_name(name: "", addr: "") = {
  rect(
    radius: 5pt,
    stroke: 0.3pt + black,
    inset: 1pt,
    grid(
      columns: (14pt, 1fr),
      gutter: 2pt,
      align: left,
      [#image("user-round.svg", width: 14pt)],
      [
        #align(horizon + left)[
          #text(name, size: 10pt)
          #v(-12pt)
          #text(addr, font: "Unifont", size: 5.5pt)
        ]
      ],
    ),
  )
}

#let account_only_addr(addr: "") = {
  rect(
    radius: 5pt,
    stroke: 0.3pt + black,
    inset: 1pt,
    grid(
      columns: (14pt, 1fr),
      gutter: 2pt,
      align: left,
      [#image("user-round.svg", width: 14pt)],
      [
        #align(horizon + left)[
          #text(addr, font: "Unifont", size: 5.5pt)
        ]
      ],
    ),
  )
}

#let account(name: "", addr: "") = {
  if name == "" {
    account_only_addr(addr: addr)
  } else {
    account_with_name(name: name, addr: addr)
  }
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

// ---

= #box(image("send.svg", width: 14pt), baseline: 3pt) 转出原生币

#align(center, text("-111.333333333 ETH", size: 12pt))

#v(-7pt)

#grid(
  gutter: 2pt,
  [#account(name: "sbchild钱包1", addr: "0x5838c07a50894fAA58EBfe5E5975f1FCc8792cD3")],
  [#flow-down(label: "Native")],
  [#account(addr: "0x5838c07a50894fAA58EBfe5E5975f1FCc8792cD3")],
)

#v(-5pt)

- 区块链: #text("Arbitrum One", weight: "bold")
- 手续费: #text("0.0000000011 ETH", weight: "bold")
- 交易:

#v(-16pt) // 把"交易:"与"0x"叠在一起

#tx(t: "0xce656824ee4da940ca08901088940d52b4fe1b5a7b4b21007daaec4f1c315529")

#v(-4pt)

#line(length: 100%, stroke: (dash: ("dot", 3pt, 3pt, 3pt), cap: "round", thickness: 0.8pt))
