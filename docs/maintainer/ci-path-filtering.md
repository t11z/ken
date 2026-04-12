# CI Path-Filtering

## Zweck

Der CI-Workflow führt nicht bei jeder Änderung alle Jobs aus. Stattdessen ermittelt
ein vorgelagerter `changes`-Job, welche Bereiche des Repository von einem Commit
betroffen sind, und nachgelagerte Jobs springen per `if:`-Bedingung nur dann an,
wenn ihr Zuständigkeitsbereich berührt wurde. Der größte praktische Gewinn liegt
bei den Windows-Runnern (ca. 2× teurer als Linux) und dem Cross-Build für
Linux/ARM64, die bei reinen Server- bzw. Agent-Änderungen nicht gegenseitig
ausgelöst werden.

## Aggregator-Invariante

Der Job `all-checks` ist der **einzige** Required Status Check, der in der
Branch-Protection konfiguriert wird. Er wartet via `needs:` auf alle anderen Jobs
und läuft mit `if: always()` auch dann, wenn vorgelagerte Jobs übersprungen wurden.
Seine Schrittlogik akzeptiert `success` und `skipped` als valide Ergebnisse;
`failure` oder `cancelled` führen zu `exit 1`.

Daraus ergibt sich die wichtigste Invariante: **Jeder neue Job muss in `needs:` von
`all-checks` eingetragen werden.** Wer einen Job hinzufügt, ohne ihn dort zu listen,
schließt ihn aus dem Required-Check-Ergebnis aus — Regressions in diesem Job
blockieren dann keinen Merge mehr. Einzelne Jobs dürfen nie als Required Status
Check in den GitHub-Settings eingetragen werden; das `skipped`-Ergebnis eines
gefilterten Jobs würde dort als Blockade gelten, weil GitHub `skipped ≠ success`
interpretiert.

## Always-Trigger-Invariante

Der `changes`-Job kennt ein Always-Trigger-Set: Dateipfade, deren Änderung dazu
führt, dass alle nachgelagerten Jobs laufen, unabhängig davon, welche anderen
Pfade ebenfalls geändert wurden. Dieses Set umfasst derzeit:

- `crates/ken-protocol/` — Shared Crate, das beide Build-Targets abhängig sind
- `Cargo.toml` (Workspace-Root) und `Cargo.lock` — eine Dependency-Änderung kann
  jeden Crate betreffen
- `rust-toolchain.toml` — eine Toolchain-Änderung kann überall Warnungen oder
  Fehler einführen
- `deny.toml` — definiert License- und Ban-Policies, die sonst nur beim nächsten
  Dependency-Commit gegen Realität geprüft würden
- `.github/workflows/ci.yml` — eine Workflow-Änderung selbst soll vollständig
  validiert werden

Änderungen an diesem Set erfordern Bedacht: zu eng gefasst bedeutet stille
Regressions, zu weit gefasst hebt den Filtereffekt auf.

## Checkliste für neue Jobs

Wer einen Job zu `ci.yml` hinzufügt, muss folgende Schritte vollständig ausführen:

1. Den neuen Job zu `needs:` von `all-checks` hinzufügen.
2. `needs: changes` und eine `if:`-Bedingung auf einen der Outputs des
   `changes`-Jobs eintragen (`always`, `server`, `agent`, `fmt` oder einen neu
   eingeführten Output).
3. Falls der Job ein bisher nicht erfasstes Crate abdeckt: im `changes`-Job einen
   neuen Output ergänzen, die Pfad-Erkennung im Shell-Skript erweitern und diese
   Dokumentation aktualisieren.
4. Prüfen, ob der neue Output auch in der Always-Trigger-Logik berücksichtigt
   werden muss (siehe unten).

## Checkliste für neue Crates

Wenn ein neues Crate angelegt wird, ist zunächst zu klären, ob es ein Shared Crate
ist (vergleichbar mit `ken-protocol`) oder einem der bestehenden Targets
zuzuordnen ist (Server oder Agent). Shared Crates müssen ins Always-Trigger-Set
aufgenommen werden, weil ihre Änderungen alle abhängigen Jobs betreffen. Crates,
die exklusiv zu einem Target gehören, erhalten einen eigenen Output im
`changes`-Job und einen neuen Pfadeintrag in der job-spezifischen Erkennungslogik.
In beiden Fällen ist diese Dokumentation anzupassen.
