# arch-health

Stato di salute di un sistema Arch Linux. Usa solo le librerie e le API di
sistema: libalpm, D-Bus, sysfs, procfs e syscall. Non lancia nessun comando esterno.

```
cargo build --release
./target/release/arch-health            # sync privato dei db + tutti i controlli
./target/release/arch-health --no-sync  # usa i db di sistema, offline
./target/release/arch-health -v         # mostra tutti i dettagli
```

Exit code: `0` ok · `1` avvisi · `2` problemi critici.

## Controlli

| Sezione   | Controllo            | Fonte |
|-----------|----------------------|-------|
| Pacchetti | Aggiornamenti        | libalpm, sync in `~/.cache/arch-health/db` (non tocca `/var/lib/pacman`) |
|           | Orfani, esterni      | libalpm, db locale |
|           | `.pacnew`            | libalpm (file di backup dei pacchetti) + filesystem |
| Riavvio   | Kernel               | `uname()` vs `/usr/lib/modules` |
|           | Librerie sostituite  | `/proc/*/maps` (tutti i processi solo da root) |
| Servizi   | Unità fallite        | D-Bus `org.freedesktop.systemd1` (sistema + utente) |
| Dischi    | Spazio / inode / ro  | `/proc/self/mounts` + `statvfs()` |
|           | Errori btrfs         | `/sys/fs/btrfs/*/devinfo/*/error_stats` |
|           | SMART / NVMe         | D-Bus `org.freedesktop.UDisks2` |

## Dipendenze

### Build (makedepends)

| Pacchetto | Perché |
|-----------|--------|
| `rust` (o `rustup`) | compilatore + cargo |
| `pkgconf` | `alpm-sys` trova `libalpm` con pkg-config |
| `pacman`  | header e `libalpm.pc` (già installato su Arch) |

### Esecuzione (depends)

| Pacchetto | Perché |
|-----------|--------|
| `pacman`  | `libalpm.so` (porta con sé curl, gpgme, libarchive, …) |
| `glibc`, `gcc-libs` | runtime Rust standard |

### Servizi usati a runtime (opzionali: se mancano, il controllo dice "non eseguito")

| Pacchetto | Perché |
|-----------|--------|
| `systemd` + `dbus` | unità fallite (sempre presenti su Arch) |
| `udisks2` | salute SMART/NVMe (attivato via D-Bus, non serve abilitarlo) |

D-Bus è implementato in Rust puro (`zbus`), quindi **non** serve `libdbus`.

## Note

- La configurazione di `pacman.conf` viene letta da un parser interno
  (`src/pacman_conf.rs`). Il crate `pacmanconf` invece esegue `pacman-conf`.
- libalpm usa internamente gpgme per verificare le firme. Sui repo ufficiali i
  database non sono firmati (`DatabaseOptional`), quindi durante il sync gpg in
  pratica non viene avviato.
