# arch-health

Stato di salute di un sistema Arch Linux. Usa solo le librerie e le API di
sistema: libalpm, D-Bus, sysfs, procfs e syscall. Non lancia nessun comando esterno.

```
cargo build --release
./target/release/arch-health            # sync privato dei db + tutti i controlli
./target/release/arch-health --no-sync  # usa i db di sistema (l'AUR viene comunque interrogato)
./target/release/arch-health --offline  # nessun accesso alla rete: niente sync, niente AUR
./target/release/arch-health -v         # mostra tutti i dettagli
```

Exit code: `0` ok · `1` avvisi · `2` problemi critici.

## Controlli

| Sezione   | Controllo            | Fonte |
|-----------|----------------------|-------|
| Pacchetti | Aggiornamenti        | libalpm, sync in `~/.cache/arch-health/db` (non tocca `/var/lib/pacman`) |
|           | Orfani, esterni      | libalpm, db locale |
|           | `.pacnew`            | libalpm (file di backup dei pacchetti) + filesystem |
|           | Aggiornamenti AUR    | pacchetti esterni (libalpm) → AUR RPC v5 `info`, confronto con `alpm::vercmp`; segnala anche pacchetti obsoleti, orfani o assenti in AUR |
| Riavvio   | Kernel               | `uname()` vs `/usr/lib/modules` |
|           | Librerie sostituite  | `/proc/*/maps` (tutti i processi solo da root) |
| Servizi   | Unità fallite        | D-Bus `org.freedesktop.systemd1` (sistema + utente) |
| Dischi    | Spazio / inode / ro  | `/proc/self/mounts` + `statvfs()` |
|           | Errori btrfs         | `/sys/fs/btrfs/*/devinfo/*/error_stats` |
|           | SMART / NVMe         | D-Bus `org.freedesktop.UDisks2` |
| Journal   | Errori dall'avvio    | sd-journal, `PRIORITY<=3` del boot corrente, raggruppati per unità |
|           | Coredump             | sd-journal, `MESSAGE_ID` di systemd-coredump, ultimi 7 giorni |

## Dipendenze

### Build (makedepends)

| Pacchetto | Perché |
|-----------|--------|
| `rust` (o `rustup`) | compilatore + cargo |
| `pkgconf` | `alpm-sys` trova `libalpm` con pkg-config |
| `pacman`  | header e `libalpm.pc` (già installato su Arch) |
| `systemd-libs` | `libsystemd` per sd-journal (già installato su Arch) |
| `openssl` | header e pkg-config per native-tls (già installato su Arch) |

### Esecuzione (depends)

| Pacchetto | Perché |
|-----------|--------|
| `pacman`  | `libalpm.so` (porta con sé curl, gpgme, libarchive, …) |
| `systemd-libs` | `libsystemd.so` (sd-journal) |
| `openssl` | TLS per le richieste HTTPS all'AUR (native-tls) |
| `ca-certificates` | certificati CA di sistema usati per HTTPS |
| `glibc`, `gcc-libs` | runtime Rust standard |

### Servizi usati a runtime (opzionali: se mancano, il controllo dice "non eseguito")

| Pacchetto | Perché |
|-----------|--------|
| `systemd` + `dbus` | unità fallite (sempre presenti su Arch) |
| `udisks2` | salute SMART/NVMe (attivato via D-Bus, non serve abilitarlo) |

D-Bus è implementato in Rust puro (`zbus`), quindi **non** serve `libdbus`.

## Note

- Il journal di sistema è leggibile solo da root o dai gruppi `wheel`, `adm`
  e `systemd-journal` (su Arch tramite ACL). Senza questi permessi viene letto
  solo il journal dell'utente, e il riepilogo lo segnala.
- Gli errori del journal con livello `err` sono solo informativi (su un desktop
  sono spesso rumore). Con livello `crit`, `alert` o `emerg` diventano critici.
- Le richieste all'AUR inviano solo i nomi dei pacchetti esterni installati
  (come fanno paru e yay), con User-Agent `arch-health/<versione>`.
- La configurazione di `pacman.conf` viene letta da un parser interno
  (`src/pacman_conf.rs`). Il crate `pacmanconf` invece esegue `pacman-conf`.
- libalpm usa internamente gpgme per verificare le firme. Sui repo ufficiali i
  database non sono firmati (`DatabaseOptional`), quindi durante il sync gpg in
  pratica non viene avviato.
