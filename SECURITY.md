# Security Policy for Continuwuity

This document outlines the security policy for Continuwuity. Our goal is to maintain a secure platform for all users, and we take security matters seriously.

## Supported Versions

We provide security updates for the following versions of Continuwuity:

| Version        | Supported        |
| -------------- |:----------------:|
| Latest release |        ✅        |
| Main branch    |        ✅        |
| Older releases |        ❌        |

We may backport fixes to the previous release at our discretion, but we don't guarantee this.

## Reporting a Vulnerability

### Responsible Disclosure

We appreciate the efforts of security researchers and the community in identifying and reporting vulnerabilities. To ensure that potential vulnerabilities are addressed properly, please follow these guidelines:

1. **Contact members of the team directly** over E2EE private message.
   - [@jade:ellis.link](https://matrix.to/#/@jade:ellis.link)
   - [@nex:nexy7574.co.uk](https://matrix.to/#/@nex:nexy7574.co.uk) <!-- ? -->
2. **Email the security team** at [security@continuwuity.org](mailto:security@continuwuity.org). This is not E2EE, so don't include sensitive details.
3. **Do not disclose the vulnerability publicly** until it has been addressed
4. **Provide detailed information** about the vulnerability, including:
   - A clear description of the issue
   - Steps to reproduce
   - Potential impact
   - Any possible mitigations
   - Version(s) affected, including specific commits if possible

If you have any doubts about a potential security vulnerability, contact us via private channels first! We'd prefer that you bother us, instead of having a vulnerability disclosed without a fix.

### What to Expect

When you report a security vulnerability:

1. **Acknowledgment**: We will acknowledge receipt of your report.
2. **Assessment**: We will assess the vulnerability and determine its impact on our users
3. **Updates**: We will provide updates on our progress in addressing the vulnerability, and may request you help test mitigations
4. **Resolution**: Once resolved, we will notify you and discuss coordinated disclosure
5. **Credit**: We will recognize your contribution (unless you prefer to remain anonymous)

## Security Update Process

When security vulnerabilities are identified:

1. We will develop and test fixes in a private fork
2. Security updates will be released as soon as possible
3. Release notes will include information about the vulnerabilities, avoiding details that could facilitate exploitation where possible
4. Critical security updates may be backported to the previous stable release

## Additional Resources

- [Matrix Security Disclosure Policy](https://matrix.org/security-disclosure-policy/)
- [Continuwuity Documentation](https://continuwuity.org/introduction)

---

This security policy was last updated on May 25, 2025.
