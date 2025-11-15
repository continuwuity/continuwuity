# Code Style Guide

This guide outlines the coding standards and best practices for Continuwuity development. These guidelines help avoid bugs and maintain code consistency, readability, and quality across the project.

These guidelines apply to new code on a best-effort basis. When modifying existing code, follow existing patterns in the immediate area you're changing and then gradually improve code style when making substantial changes.

## General Principles

- **Clarity over cleverness**: Write code that is easy to understand and maintain
- **Consistency**: Pragmatically follow existing patterns in the codebase, rather than adding new dependencies.
- **Safety**: Prefer safe, explicit code over unsafe code with implicit requirements
- **Performance**: Consider performance implications, but not at the expense of correctness or maintainability

## Formatting and Linting

All code must satisfy lints (clippy, rustc, rustdoc, etc) and be formatted using **nightly** rustfmt (`cargo +nightly fmt`). Many of the `rustfmt.toml` features depend on the nightly toolchain.

If you need to allow a lint, ensure it's either obvious why (e.g. clippy saying redundant clone but it's actually required) or add a comment explaining the reason. Do not write inefficient code just to satisfy lints. If a lint is wrong and provides a less efficient solution, allow the lint and mention that in a comment.

If making large formatting changes across unrelated files, create a separate commit so it can be added to the `.git-blame-ignore-revs` file.

## Rust-Specific Guidelines

### Naming Conventions

Follow standard Rust naming conventions as outlined in the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/naming.html):

- Use `snake_case` for functions, variables, and modules
- Use `PascalCase` for types, traits, and enum variants
- Use `SCREAMING_SNAKE_CASE` for constants and statics
- Use descriptive names that clearly indicate purpose

```rs
// Good
fn process_user_request(user_id: &UserId) -> Result<Response, Error> { ... }

const MAX_RETRY_ATTEMPTS: usize = 3;

struct UserSession {
    session_id: String,
    created_at: SystemTime,
}

// Avoid
fn proc_reqw(id: &str) -> Result<Resp, Err> { ... }
```

### Error Handling

- Use `Result<T, E>` for operations that can fail
- Prefer specific error types over generic ones
- Use `?` operator for error propagation
- Provide meaningful error messages
- If needed, create or use an error enum.

```rs
// Good
fn parse_server_name(input: &str) -> Result<ServerName, InvalidServerNameError> {
    ServerName::parse(input)
        .map_err(|_| InvalidServerNameError::new(input))
}

// Avoid
fn parse_server_name(input: &str) -> Result<ServerName, Box<dyn Error>> {
    Ok(ServerName::parse(input).unwrap())
}
```

### Option Handling

- Prefer explicit `Option` handling over unwrapping
- Use combinators like `map`, `and_then`, `unwrap_or_else` when appropriate

```rs
// Good
let display_name = user.display_name
    .as_ref()
    .map(|name| name.trim())
    .filter(|name| !name.is_empty())
    .unwrap_or(&user.localpart);

// Avoid
let display_name = if user.display_name.is_some() {
    user.display_name.as_ref().unwrap()
} else {
    &user.localpart
};
```

## Logging Guidelines

### Structured Logging

**Always use structured logging instead of string interpolation.** This improves log parsing, filtering, and observability.

```rs
// Good - structured parameters
debug!(
    room_id = %room_id,
    user_id = %user_id,
    event_type = ?event.event_type(),
    "Processing room event"
);

info!(
    server_name = %server_name,
    response_time_ms = response_time.as_millis(),
    "Federation request completed successfully"
);

// Avoid - string interpolation
debug!("Processing room event for {room_id} from {user_id}");
info!("Federation request to {server_name} took {response_time:?}");
```

### Log Levels

Use appropriate log levels:

- `error!`: Unrecoverable errors that affect functionality
- `warn!`: Potentially problematic situations that don't stop execution
- `info!`: General information about application flow
- `debug!`: Detailed information for debugging
- `trace!`: Very detailed information, typically only useful during development

Keep in mind the frequency that the log will be reached, and the relevancy to a server operator.

```rs
// Good
error!(
    error = %err,
    room_id = %room_id,
    "Failed to send event to room"
);

warn!(
    server_name = %server_name,
    attempt = retry_count,
    "Federation request failed, retrying"
);

info!(
    user_id = %user_id,
    "User registered successfully"
);

debug!(
    event_id = %event_id,
    auth_events = ?auth_event_ids,
    "Validating event authorization"
);
```

### Sensitive Information

Never log sensitive information such as:
- Access tokens
- Passwords
- Private keys
- Personal user data (unless specifically needed for debugging)

```rs
// Good
debug!(
    user_id = %user_id,
    session_id = %session_id,
    "Processing authenticated request"
);

// Avoid
debug!(
    user_id = %user_id,
    access_token = %access_token,
    "Processing authenticated request"
);
```

## Lock Management

### Explicit Lock Scopes

**Always use closure guards instead of implicitly dropped guards.** This makes lock scopes explicit and helps prevent deadlocks.

Use the `WithLock` trait from `core::utils::with_lock`:

```rs
use conduwuit::utils::with_lock::WithLock;

// Good - explicit closure guard
shared_data.with_lock(|data| {
    data.counter += 1;
    data.last_updated = SystemTime::now();
    // Lock is explicitly released here
});

// Avoid - implicit guard
{
    let mut data = shared_data.lock().unwrap();
    data.counter += 1;
    data.last_updated = SystemTime::now();
    // Lock released when guard goes out of scope - less explicit
}
```

For async contexts, use the async variant:

```rs
use conduwuit::utils::with_lock::WithLockAsync;

// Good - async closure guard
async_shared_data.with_lock(|data| {
    data.process_async_update();
}).await;
```

### Lock Ordering

When acquiring multiple locks, always acquire them in a consistent order to prevent deadlocks:

```rs
// Good - consistent ordering (e.g., by memory address or logical hierarchy)
let locks = [&lock_a, &lock_b, &lock_c];
locks.sort_by_key(|lock| lock as *const _ as usize);

for lock in locks {
    lock.with_lock(|data| {
        // Process data
    });
}

// Avoid - inconsistent ordering that can cause deadlocks
lock_b.with_lock(|data_b| {
    lock_a.with_lock(|data_a| {
        // Deadlock risk if another thread acquires in A->B order
    });
});
```

## Documentation

### Code Comments

- Reference related documentation or parts of the specification
- When a task has multiple ways of being achieved, explain your reasoning for your decision
- Update comments when code changes

```rs
/// Processes a federation request with automatic retries and backoff.
///
/// Implements exponential backoff to handle temporary
/// network issues and server overload gracefully.
pub async fn send_federation_request(
    destination: &ServerName,
    request: FederationRequest,
) -> Result<FederationResponse, FederationError> {
    // Retry with exponential backoff because federation can be flaky
    // due to network issues or temporary server overload
    let mut retry_delay = Duration::from_millis(100);

    for attempt in 1..=MAX_RETRIES {
        match try_send_request(destination, &request).await {
            Ok(response) => return Ok(response),
            Err(err) if err.is_retriable() && attempt < MAX_RETRIES => {
                warn!(
                    destination = %destination,
                    attempt = attempt,
                    error = %err,
                    retry_delay_ms = retry_delay.as_millis(),
                    "Federation request failed, retrying"
                );

                tokio::time::sleep(retry_delay).await;
                retry_delay *= 2; // Exponential backoff
            }
            Err(err) => return Err(err),
        }
    }

    unreachable!("Loop should have returned or failed by now")
}
```

### Async Patterns

- Use `async`/`await` appropriately
- Avoid blocking operations in async contexts
- Consider using `tokio::task::spawn_blocking` for CPU-intensive work

```rs
// Good - non-blocking async operation
pub async fn fetch_user_profile(
    &self,
    user_id: &UserId,
) -> Result<UserProfile, Error> {
    let profile = self.db
        .get_user_profile(user_id)
        .await?;

    Ok(profile)
}

// Good - CPU-intensive work moved to blocking thread
pub async fn generate_thumbnail(
    &self,
    image_data: Vec<u8>,
) -> Result<Vec<u8>, Error> {
    tokio::task::spawn_blocking(move || {
        image::generate_thumbnail(image_data)
    })
    .await
    .map_err(|_| Error::TaskJoinError)?
}
```

## Inclusivity and Diversity Guidelines

All code and documentation must be written with inclusivity and diversity in mind. This ensures our software is welcoming and accessible to all users and contributors. Follow the [Google guide on writing inclusive code and documentation](https://developers.google.com/style/inclusive-documentation) for comprehensive guidance.

The following types of language are explicitly forbidden in all code, comments, documentation, and commit messages:

**Ableist language:** Avoid terms like "sanity check", "crazy", "insane", "cripple", or "blind to". Use alternatives like "validation", "unexpected", "disable", or "unaware of".

**Socially-charged technical terms:** Replace overly divisive terminology with neutral alternatives:
- "whitelist/blacklist" → "allowlist/denylist" or "permitted/blocked"
- "master/slave" → "primary/replica", "controller/worker", or "parent/child"

When working with external dependencies that use non-inclusive terminology, avoid propagating them in your own APIs and variable names.

Use diverse examples in documentation that avoid culturally-specific references, assumptions about user demographics, or unnecessarily gendered language. Design with accessibility and inclusivity in mind by providing clear error messages and considering diverse user needs.

This software is intended to be used by everyone regardless of background, identity, or ability. Write code and documentation that reflects this commitment to inclusivity.
