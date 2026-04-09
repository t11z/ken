name: 🏛️ ADR proposal
description: Propose a new Architecture Decision Record or a change to a Tier 2 boundary
labels: ["type/adr", "status/needs-discussion"]
body:
  - type: markdown
    attributes:
      value: |
        An ADR records a significant, load-bearing decision about Ken. This template is for proposing a new ADR or a Tier 2 scope change.

        If you are unsure whether your idea needs an ADR, it probably does. Small behavior tweaks and bug fixes do not need ADRs, but anything that changes *what Ken does* or *what Ken refuses to do* does.

        Read [ADR-0000](https://github.com/t11z/ken/blob/main/docs/adr/0000-adr-format-and-lifecycle.md) for the ADR format itself, and the [writing-adrs skill](https://github.com/t11z/ken/blob/main/.claude/skills/writing-adrs/SKILL.md) for guidance on writing a good one.

  - type: input
    id: title
    attributes:
      label: Proposed ADR title
      description: |
        Imperative, concrete, and specific. The title alone should tell a reader what the decision is.
      placeholder: e.g. "Allow consent-gated Defender quick scans"
    validations:
      required: true

  - type: textarea
    id: context
    attributes:
      label: Context
      description: |
        What situation or pressure is forcing this decision? What is true today that makes the status quo insufficient?
    validations:
      required: true

  - type: textarea
    id: decision
    attributes:
      label: Proposed decision
      description: |
        In one or two paragraphs, what do you propose? Use declarative language — "Ken does X," not "Ken should do X."
    validations:
      required: true

  - type: dropdown
    id: touches
    attributes:
      label: Does this ADR supersede an existing one?
      options:
        - No, this is a new ADR on a topic not yet decided
        - Yes, it supersedes an existing ADR (list the ADR number below)
        - It loosens a Tier 2 boundary in ADR-0001
    validations:
      required: true

  - type: input
    id: supersedes
    attributes:
      label: ADR number this supersedes (if applicable)
      placeholder: e.g. ADR-0007

  - type: textarea
    id: consequences
    attributes:
      label: Consequences
      description: |
        What becomes easier, what becomes harder, what are you explicitly accepting? All three are required.
    validations:
      required: true

  - type: textarea
    id: alternatives
    attributes:
      label: Alternatives considered
      description: |
        At least one rejected alternative with a substantive reason. "We also thought about X but it was harder" is not enough.
    validations:
      required: true

  - type: checkboxes
    id: tier-1-check
    attributes:
      label: Tier 1 check
      description: Confirm that this proposal does not violate any Tier 1 invariant in ADR-0001.
      options:
        - label: I have read ADR-0001 Tier 1 and this proposal does not loosen any Tier 1 invariant.
          required: true
