---
description: "Fenced code with a language is highlighted with classes, in both colour schemes."
---
# Code

A fenced block that names a language is highlighted when the language is known. The colours come
from the theme, in both colour schemes. A block with an unknown language, or without one, is shown
as plain text. Blocks over 100 KB are never highlighted.

````markdown
```rust
fn main() {
    println!("hello");
}
```
````

```rust
// Count the words of a line.
fn words(line: &str) -> usize {
    let n: usize = line.split_whitespace().count();
    println!("{n} words in {:?}", line);
    n
}
```

```python
def words(line: str) -> int:
    """Count the words of a line."""
    return len(line.split())
```

```javascript
// Count the words of a line.
const words = (line) => line.split(/\s+/).filter(Boolean).length;
```

```bash
# Build and check the library.
anaquel build --strict && echo "built"
```

```json
{ "name": "anaquel", "stable": false, "versions": [1, 2, 3] }
```

```html
<p class="note">Escaped, never run: <b>text</b></p>
```

```css
.hint { border-inline-start: 0.25rem solid #2563a8; }
```

A language that is not known is left plain:

```notalanguage
fn main() {}
```
