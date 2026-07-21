# Security Policy for Continuwuity

This document outlines the security policy for Continuwuity. Our goal is to maintain a secure platform for all users,
and we take security matters seriously.

## Supported Versions

We provide security updates for the following versions of Continuwuity:

| Version        | Supported |
|----------------|:---------:|
| Latest release |     ✅     |
| Main branch    |     ✅     |
| Older releases |     ❌     |

We may backport fixes to the previous release at our discretion, but we don't guarantee this; our versioning is designed
to encourage users to stay up-to-date with the latest release, and we have no concept of "long term support".

## Reporting a Vulnerability

### Responsible Disclosure

We appreciate the efforts of security researchers and the community in identifying and reporting vulnerabilities. To
ensure that potential vulnerabilities are addressed properly, please follow these guidelines:

1. **Contact members of the team directly** over E2EE private message.
    - [@jade:ellis.link](https://matrix.to/#/@jade:ellis.link)
    - [@nex:nexy7574.co.uk](https://matrix.to/#/@nex:nexy7574.co.uk)
    - [@ginger:gingershaped.computer](https://matrix.to/#/@ginger:gingershaped.computer)
2. **Email the security team** at [security@continuwuity.org](mailto:security@continuwuity.org). This is not E2EE, so
   don't include sensitive details.
3. **Do not disclose the vulnerability publicly** until a fix has been pushed to the main branch.
4. **Provide detailed information** about the vulnerability, including:
    - A clear description of the issue
    - Steps to reproduce
    - Potential impact
    - Any possible mitigations
    - Version(s) affected, including specific commits if possible
    - How you want to be attributed if your report is accepted (website, social media handle, Matrix user ID, etc).
      **Please state explicitly if you wish to remain anonymous.**

If you have any doubts about a potential security vulnerability, contact us via private channels first! We'd prefer that
you bother us, instead of having a vulnerability disclosed without a fix.

### Terms for credit

Before reporting a vulnerability, please remember that we are a small team maintaining a large codebase depended upon by
a vast unknown number of users in our free time. While we always investigate *all* security reports, we may not
acknowledge or credit your report under certain circumstances.

#### Following the security policy

If you do not report the vulnerability following this security policy, your report may be ignored and/or may not be
credited if fixed. This includes filing for GitHub security advisories without contacting us directly first (we do not
get notified about these!).

#### Automation-assisted reports

Reports assisted by automatic tooling such as LLMs MUST disclose such in the report.
Assisted reports must also produce a working proof-of-concept (PoC) demonstrating the vulnerability against the latest
release and/or main commit with no modifications to the codebase. This is to demonstrate that not only does
the author understand the vulnerability that they are reporting, but also that the vulnerability is reproducible
and not a false positive.

#### Reports for known issues

Reports for issues we are already aware of will only be credited if subsequent reporters provide new information.

#### Audits and CVE farming

Sweeping audits (vulnerability hunting) **must** be coordinated with the team first - receiving a rapsheet of new
vulnerabilities is explicitly not helpful to us and only harms the project. If you are interested in performing a
security audit, please contact us first to discuss the scope and methodology.

Likewise, CVE farming (reporting vulnerabilities with the primary intent to get a CVE number) is actively harmful to the
project, and will not be credited. Severe violations of this policy will result in a permanent ban from collaborating
with the project in any capacity. We are attempting to build high quality free software, not flesh out your CV/resume.

### What to expect

When you report a security vulnerability:

1. **Acknowledgment**: We will acknowledge receipt of your report. We may ask for further information.
2. **Triage**: The report will be triaged into our internal tracker, and you will be provided with a reference number (
   in case you end up with multiple reports). An ETA for a fix will be provided if feasible.
3. **Updates**: We will provide updates on our progress in addressing the vulnerability, including a heads-up for when
   we plan to release a fix.
4. **Resolution**: Once resolved, we will notify you and discuss coordinated disclosure
5. **Credit**: We will recognize your contribution (unless you prefer to remain anonymous)

## Security Update Process

When security vulnerabilities are identified:

1. We will develop and test fixes in a private fork
2. Security updates will be released as soon as possible
3. Release notes will include information about the vulnerabilities, avoiding details that could facilitate exploitation
   where possible
4. Critical security updates may be backported to the previous stable release

## Additional Resources

- [Matrix Security Disclosure Policy](https://matrix.org/security-disclosure-policy/)
- [Continuwuity Documentation](https://continuwuity.org/introduction)

---

This security policy was last updated on May 25, 2025.
