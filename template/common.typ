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
      [#image("user-round-star.svg", width: 14pt)],
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
) = {
  layout(container-size => context {
    let max-width = container-size.width
    let current-size = max-size

    let text-width = measure(text(size: current-size, top-edge: "bounds")[#body]).width

    while text-width > max-width and current-size > min-size {
      current-size -= step
      text-width = measure(text(size: current-size, top-edge: "bounds")[#body]).width
    }

    text(size: current-size)[#body]
  })
}
