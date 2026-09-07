# markatui

A live-preview Markdown editor. The block under the cursor shows its **raw source**; every other block stays rendered, so the file on disk is exactly what you typed.

## Lists

- First item
- Second item
  - Nested item
- Third item

1. First
2. Second

## Code

```rust
fn main() {
    println!("hello");
}
```

## Quote

> Writing is thinking. To write well is to think clearly. That's why it's so hard.

## Table

| Block | Renders as |
| --- | --- |
| heading | large text |
| code | monospace |

---

Inline `code`, a [link](https://example.com), *emphasis* and ~~strikethrough~~.

![A picture](image.png)
