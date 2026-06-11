# Markdown Test Document

This document exercises all features of the markdown renderer.

---

## 1. Headings

# H1 Heading
## H2 Heading
### H3 Heading
#### H4 Heading
##### H5 Heading
###### H6 Heading

---

## 2. Paragraphs and Inline Formatting

This is a normal paragraph with **bold text**, *italic text*, ~~strikethrough~~, and `inline code`.

You can also do ***bold and italic***, ~~**bold strikethrough**~~, and `code with **bold** inside` (depends on parser).

Here's a [link to OpenAI](https://openai.com) and another [link with **bold** text](https://example.com).

---

## 3. Code Blocks

### Rust
```rust
fn main() {
    let message = "Hello, world!";
    println!("{}", message);
    
    // This is a comment
    let numbers = vec![1, 2, 3, 4, 5];
    let sum: i32 = numbers.iter().sum();
    
    match sum {
        15 => println!("Correct!"),
        _ => println!("Wrong!"),
    }
}
```

### Python
```python
def fibonacci(n):
    if n <= 1:
        return n
    return fibonacci(n-1) + fibonacci(n-2)

# Generate first 10 numbers
for i in range(10):
    print(f"F({i}) = {fibonacci(i)}")
```

### JavaScript
```javascript
async function fetchData() {
    try {
        const response = await fetch('/api/data');
        const data = await response.json();
        console.log('Received:', data);
    } catch (error) {
        console.error('Failed:', error);
    }
}
```

### Diff
```diff
  fn main() {
-     let x = 10;
+     let x = 20;
      println!("{}", x);
  }
```

### Plain (no language)
```
This is a plain code block.
No syntax highlighting here.
Just raw text.
```

### Very long line
```python
x = "This is an extremely long line that should test whether the code block supports horizontal scrolling or if it just wraps awkwardly across multiple lines which would make it hard to read wide code like this string"
```

---

## 4. Blockquotes

> This is a blockquote.
> It can span multiple lines.

> **Bold quote** with `code` inside.
> 
> And a second paragraph in the quote.

> > Nested blockquote.
> > This is level 2.
> >
> > > Deep nested blockquote (level 3).
> > > Can the renderer show different styling per level?

---

## 5. Lists

### Unordered
- First item
- Second item
- Third item with **bold**
- Fourth item with `code`

### Nested unordered
- Parent item
  - Child item 1
  - Child item 2
    - Grandchild item
- Back to parent level

### Ordered
1. First step
2. Second step
3. Third step

### Ordered with offset
5. This starts at 5
6. Should continue as 6
7. And this is 7

### Task list
- [x] Completed task
- [ ] Incomplete task
- [x] Another completed task with **bold**

### Mixed formatting in lists
1. Item with *italic*
2. Item with `code` and [a link](https://example.com)
3. Item with ~~strikethrough~~

---

## 6. Tables

### Basic table
| Name | Age | Role |
|------|-----|------|
| Alice | 30 | Admin |
| Bob | 25 | User |
| Carol | 35 | Editor |

### Table with inline formatting
| Feature | Status | Notes |
|---------|--------|-------|
| **Bold text** | Working | Should be bold |
| *Italic text* | Working | Should be italic |
| `Code` | Working | Should be code |
| ~~Strikethrough~~ | Working | Should be struck |
| [Link](https://example.com) | Working | Should be link-colored |

### Aligned table
| Left | Center | Right |
|:-----|:------:|------:|
| L1 | C1 | R1 |
| L2 | C2 | R2 |
| L3 | C3 | R3 |

### Table with code blocks
| Language | Example |
|----------|---------|
| Rust | `fn main() {}` |
| Python | `def main(): pass` |
| JS | `function main() {}` |

---

## 7. Horizontal Rules

Above the rule.

---

Below the rule.

***

Another rule with asterisks.

---

## 8. Images

![Alt text for the image](https://via.placeholder.com/150)

Image with link: [![Clickable image](https://via.placeholder.com/100)](https://example.com)

---

## 9. HTML

Some inline HTML: <span style="color: red">this should show as text</span>.

<details>
<summary>Click to expand</summary>
This content is hidden inside HTML details.
</details>

---

## 10. Edge Cases

### Empty elements

> 

- 

### Tight list (no blank lines between items)
- **2015** — Rust 1.0
- **2018** — Rust 1.31 (Dec 2018)
- **2021** — Rust 1.56 (Oct 2021)
- **2024** — Rust 1.85 (Feb 2025)

### Escaping
\*not italic\* \`not code\* \# not heading

### Special characters
Emoji: 🎉 🚀 💻
Symbols: © ® ™ € £ ¥
Math: ∑ ∏ ∫ ∂ √

### Mixed block elements
> This quote contains a list:
> - Item A
> - Item B
> 
> And a code block:
> ```
> code in quote
> ```

---

## 11. Complex Nested Structure

1. **First ordered item**
   > Blockquote inside list item
   > With **bold** and `code`
   > 
   > | Col1 | Col2 |
   > |------|------|
   > | A | B |
   > | C | D |
   
   - Nested unordered item
   - Another nested item with [link](https://example.com)

2. **Second ordered item**
   ```python
   # Code in list item
   print("Hello from list!")
   ```

---

*End of test document.*
