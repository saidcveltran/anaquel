# Security policy

## Reporting a vulnerability

Please do not open a public issue for a security problem. Use GitHub's private
vulnerability reporting: the **Security** tab of the repository, then **Report a
vulnerability**. Include the version, the steps to reproduce and the impact.

Expect an acknowledgement within a week. The project is maintained by one
person, so fixes are best effort and not tied to a fixed deadline.

## What counts

- Content that makes the engine write outside the output folder, run code, or
  emit HTML that bypasses the Content-Security-Policy.
- A preview server reachable from another machine or from a web page.
- A crash or runaway use of memory or time on hostile input.

## Supported versions

Only the latest release receives fixes while the project is before 1.0.

## Design notes

- Raw HTML in content is refused; link schemes are limited to `http`, `https`
  and `mailto`.
- The generated `_headers` forbids inline scripts and styles.
- The preview server binds to `127.0.0.1` only and checks the `Host` header.
