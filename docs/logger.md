# Logger Usage

## Setup

At app startup, create the log panel and logger together:

```rust
let (log_panel, logger) = LogPanel::new();
```

## Logging Messages

Clone and pass `logger` to whatever components need it:

```rust
let logger2 = logger.clone();

// Log from anywhere
logger.debug("Something happened");
logger2.debug(format!("Value is {}", x));
```

## Render Loop

Call `poll()` each frame to drain pending messages into the panel:

```rust
log_panel.poll();
```

## Adding More Log Levels (Optional)

You can extend `Logger` with more methods:

```rust
impl Logger {
    pub fn info(&self, message: impl Into<String>) { ... }
    pub fn warn(&self, message: impl Into<String>) { ... }
    pub fn error(&self, message: impl Into<String>) { ... }
}
```
