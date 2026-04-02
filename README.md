# 0-Bytes OS

Un système d'exploitation où **aucun fichier ne contient jamais de données**.

Toute l'information est encodée dans les **noms** de fichiers et dossiers, la **hiérarchie** des répertoires, l'**existence** des fichiers et les **métadonnées** (horodatages). Le système de fichiers EST l'ordinateur. Chaque `touch` est une instruction CPU, chaque `rm` est une désallocation mémoire, chaque `mv` est une transformation de données, chaque `mkdir` est une allocation mémoire.

Un moteur Rust appelé **Pith** observe le système de fichiers, l'interprète comme un système vivant, et l'expose à tout programme dans n'importe quel langage.

## Pourquoi

- **0 espace de stockage requis** — Aucune donnée n'est jamais écrite dans un fichier. Toute l'information vit dans les noms, les chemins et les métadonnées. Stocker et manipuler des données ne coûte littéralement aucun octet de contenu disque.
- **Protocole de communication low-level sur la machine hôte** — Le canal de communication est le système de fichiers lui-même, via un socket Unix. Pas de serveur HTTP, pas de framework, pas de sérialisation lourde. Juste `touch`, `rm`, `mv`, `mkdir` — les primitives les plus basses du noyau. N'importe quel langage disposant d'I/O fichier peut interagir nativement.
- **Un système d'exploitation virtuel, discret et inhabituel** — 0-Bytes vit entièrement dans un répertoire. Il ne laisse aucune trace conventionnelle : pas de processus suspect, pas de binaire exotique en mémoire, pas de port réseau ouvert. Son fonctionnement repose sur des opérations de fichiers banales, invisibles dans un audit classique.
- **Parfait pour les opérations database haute performance, haute discrétion d'empreinte système** — Les requêtes sont des traversées de trie en mémoire. Pas de moteur SQL, pas de daemon lourd. L'empreinte système est quasi nulle : quelques fichiers vides et un processus Rust léger. Idéal pour des scénarios où la performance et la discrétion sont essentielles.

## Les quatre primitives

| Commande | Sémantique OS |
|----------|---------------|
| `touch`  | Affirmer / Signaler / Allouer un bit |
| `rm`     | Rétracter / Désallouer / Nier |
| `mv`     | Transformer / Renommer / Réassigner |
| `mkdir`  | Allouer une portée / Ouvrir un espace de noms |

Il n'existe aucun autre moyen de changer l'état.

## Trois classes de nœuds

Chaque entrée du système de fichiers est classifiée selon le début de son nom :

```
blue          → Nœud de données    (le nom EST la valeur)
-expected     → Nœud d'instruction (- est une porte logique : "pointeur d'état")
€$price       → Nœud pointeur     (€ échappe : "$price" littéral, pas un schéma)
```

## Portes logiques

Les portes logiques sont des caractères réservés qui agissent comme des fonctions de transformation. L'alphabet est **auto-descriptif** : le moteur lit `src/hard/reserved/` au démarrage. Ajoutez un fichier, étendez le langage.

`€` (U+20AC) est la seule valeur codée en dur — l'**axiome**. Il échappe le caractère suivant de l'interprétation comme porte logique.

| Car. | Nom | Car. | Nom | Car. | Nom |
|------|-----|------|-----|------|-----|
| `€` | Échappement | `$` | Schéma | `-` | État |
| `!` | Signal | `#` | Canal | `§` | Permission |
| `~` | Nombre | `@` | Clé Dict | `:` | Liaison |
| `[` `]` | Tableau | `{` `}` | Objet | `(` `)` | Valeur brute |
| `*` | Compilé | `+` | Constante | `\|` | Sép. valeur |
| `,` | Sép. objet | `_` | Joker | `^` | Priorité |
| `&` | Async | `?` | Requête | `%` | Modulo |
| `<` | Entrée | `>` | Sortie | `=` | Assertion |
| `;` | Séquence | `¶` | Processus | `∂` | Delta |
| `λ` | Lambda | `∴` | Alors | `∵` | Parce que |
| `∞` | Boucle | `▶` | Démarrer | `⏸` | Pause |
| `⏹` | Arrêter | `⌚` | Minuteur | | |

## Le chemin comme phrase

Un chemin se lit de gauche à droite comme une phrase :

```
src/hard/identities/001/-expected/type/identity
     │         │     │      │       │      │
   portée    portée slot  état :  portée  feuille
                          attendu
```

> « Dans le système hard, identités, slot 001, à l'état attendu, de type identité. »

## Organisation du système de fichiers

```
src/
├── hard/                    # ROM — définitions système immuables
│   ├── reserved/            # 38 fichiers de portes logiques (l'alphabet)
│   ├── identities/          # Slots d'identité (illimités)
│   ├── groups/              # Groupes de permissions (system, admin, developers, guests)
│   └── types/               # Définitions de types (identity, job, worker, program, ...)
├── states/                  # Machine à états globale
├── jobs/                    # File d'attente de tâches (cycle : pending → running → completed)
├── workers/                 # Pool de workers
├── channels/                # Files de messages IPC (#system, #errors)
├── events/                  # Signaux fire-and-forget (!boot, !shutdown, ...)
├── programs/                # Programmes installés (machines à états sous forme d'arborescences)
├── databases/               # Données sémantiques dans des hiérarchies de chemins
├── pointers/                # Tables de référence (65 536 points de code Unicode)
├── schedules/               # Tâches planifiées (mtime = prochaine exécution)
├── sessions/                # Sessions API actives
├── subscriptions/           # Abonnements aux événements par identité
├── logs/                    # Entrées de journal horodatées
└── tmp/                     # Espace temporaire (nettoyé au démarrage)
```

## Pith — Le moteur Rust

Pith observe le système de fichiers et réagit. Il n'exécute pas de programmes — il interprète les changements du système de fichiers comme des instructions.

### Architecture

```
Système de fichiers (le matériel)
        │
        │ kqueue / inotify
        ▼
   ┌─────────┐
   │ Watcher  │  surveille 11 portées récursivement
   └────┬────┘
        │
   ┌────▼────┐
   │ Parser   │  classifie les segments (Data/Instruction/Pointer)
   └────┬────┘
        │
   ┌────▼──────┐
   │ Dispatcher │  route par portée, met à jour le trie en mémoire
   └──┬──┬──┬──┘
      │  │  │
      ▼  ▼  ▼
   10 sous-systèmes   events, channels, logs, states, jobs,
                      workers, scheduler, programs, databases,
                      subscriptions
      │  │  │
      ▼  ▼  ▼
   ┌──────────┐
   │ Effector  │  touch / rm / mv / mkdir (avec évitement de boucles)
   └──────────┘
```

### Démarrage rapide

```bash
cd pith
cargo build
cargo run -- start --root ../src
```

Pith démarre, charge les 38 portes logiques, construit un trie en mémoire d'environ 3200 nœuds, charge 777 identités et 4 groupes de permissions, commence à surveiller le système de fichiers, ouvre une API sur un socket Unix à `/tmp/pith.sock`, et entre dans sa boucle d'événements.

### API

Pith expose une API JSON délimitée par des retours à la ligne via un socket de domaine Unix.

```python
import socket, json

def pith(op, path="", args=None):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect("/tmp/pith.sock")
    req = {"op": op, "path": path}
    if args: req["args"] = args
    s.sendall((json.dumps(req) + "\n").encode())
    data = b""
    while b"\n" not in data:
        data += s.recv(4096)
    s.close()
    return json.loads(data)

pith("ping")                            # → {"ok": true, "data": "pong"}
pith("status")                          # → {"ok": true, "data": {"status": "running", "nodes": 3228}}
pith("ls", "hard/types")                # → ["channel","database","event","identity","job","program","schema","worker"]
pith("touch", "events/!hello")          # crée le fichier signal
pith("rm", "events/!hello")             # le supprime
pith("db_query", "colors")              # → ["∆psychology∆blue"]
pith("mv", "tmp/a", {"to": "tmp/b"})   # renomme a en b
```

**Opérations :** `ping`, `status`, `ls`, `query`, `touch`, `mkdir`, `rm`, `mv`, `db_query`

Comme le protocole est le système de fichiers, tout langage disposant d'I/O fichier peut aussi interagir directement :

```bash
touch src/events/'!my_signal'     # émettre un signal
rm src/events/'!my_signal'        # le rétracter
mkdir -p src/jobs/1/-state        # créer une tâche
touch src/jobs/1/-state/pending   # définir son état
```

## Système de permissions

Les permissions sont encodées dans le système de fichiers via la porte logique `§`. Pas de chmod/chown Unix — une surcouche personnalisée appliquée par Pith.

```
src/hard/identities/001/
    -group/system              # appartenance au groupe
    §read/_                    # peut tout lire (joker)

src/hard/groups/developers/
    §read/databases            # peut lire databases/
    §write/jobs                # peut écrire dans jobs/
    §execute/workers           # peut exécuter des workers

src/hard/groups/guests/
    §read/databases            # peut lire databases/
    §deny/hard                 # accès explicitement refusé à hard/
```

Résolution : **deny > own > grant > deny par défaut**.

Niveaux d'identité dérivés du premier chiffre : 0xx=omni, 1xx=shadow, 2xx=superroot, 3xx=root, 4xx=admin, 5xx=permissioned, 6xx=user, 7xx=shared, 8xx=guest, 9xx=digitalconsciousness.

## Documentation

| Document | Contenu |
|----------|---------|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Vision, primitives, boucle d'événements, démarrage/arrêt |
| [docs/RESERVED_VALUES.md](docs/RESERVED_VALUES.md) | Alphabet complet des portes logiques, mécanisme d'échappement `€` |
| [docs/NAMING.md](docs/NAMING.md) | Grammaire de nommage, classification des segments, chemin-comme-phrase |
| [docs/PERMISSIONS.md](docs/PERMISSIONS.md) | Modèle d'identité, verbes `§`, algorithme de résolution |
| [docs/FILESYSTEM.md](docs/FILESYSTEM.md) | Organisation complète du système de fichiers, stratégie de mise à l'échelle |

## Structure du projet

```
0-bytes/
├── src/              # Le système de fichiers 0-bytes OS (fichiers de zéro octet uniquement)
├── docs/             # Documentation d'architecture
├── pith/             # Le moteur Rust
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs           # Point d'entrée CLI
│       ├── alphabet.rs       # Chargeur de portes logiques auto-descriptif
│       ├── parser.rs         # Classificateur de segments
│       ├── trie.rs           # Index du système de fichiers en mémoire
│       ├── identity.rs       # Identité + niveaux de privilèges
│       ├── permissions.rs    # Moteur de permissions
│       ├── watcher.rs        # Surveillant du système de fichiers
│       ├── dispatcher.rs     # Routage d'événements + mises à jour du trie
│       ├── effector.rs       # Écrivain du système de fichiers
│       ├── api/              # Serveur socket Unix
│       └── subsystems/       # 10 sous-systèmes réactifs
└── .gitmodules       # sous-modules pointers + databases
```

**28 fichiers source Rust. 86 tests. ~1600 lignes d'implémentation.**

## Licence

À déterminer
