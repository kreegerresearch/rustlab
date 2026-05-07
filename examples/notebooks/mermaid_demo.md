---
title: Mermaid Diagrams Demo
---

# Mermaid Diagrams

Pure-Rust Mermaid rendering. Diagrams are rendered server-side to SVG —
HTML embeds inline, LaTeX/PDF reference an SVG file, Markdown emits the
verbatim ` ```mermaid ` fence so GitHub and Obsidian render it
themselves.

## Flowchart

A simple signal-processing pipeline:

<!-- caption: ADC → FIR → decimator → buffer -->
```mermaid
flowchart LR
  A[ADC samples] --> B(FIR filter)
  B --> C{Decimate?}
  C -->|yes| D[Downsample x4]
  C -->|no| E[Pass through]
  D --> F[Output buffer]
  E --> F
```

## Sequence diagram

```mermaid
sequenceDiagram
  participant U as User
  participant N as Notebook
  participant R as Renderer
  U->>N: render foo.md
  N->>R: render mermaid block
  R-->>N: SVG bytes
  N-->>U: foo.html
```

## Collapsible diagram

The next diagram lives behind a disclosure widget:

<!-- details: State machine -->
```mermaid
stateDiagram-v2
  [*] --> Idle
  Idle --> Recording: start
  Recording --> Idle: stop
  Recording --> Paused: pause
  Paused --> Recording: resume
```

## Inline rustlab + diagram

Code blocks and diagrams compose:

```rustlab
N = 64;
M = N / 4;
print("Decimation: ${N} samples → ${M} samples")
```

```mermaid
flowchart LR
  S[64 samples] --> P[/4 decimator/] --> O[16 samples]
```
