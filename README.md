<div align="center">

# Claudex

**Le moteur Codex CLI, avec la compatibilité Claude Code intégrée nativement.**

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Base](https://img.shields.io/badge/base-openai%2Fcodex%200.160.0-8A2BE2)](https://github.com/openai/codex)
[![Language](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20x64-0078D6)](#construction-depuis-les-sources)
[![Status](https://img.shields.io/badge/status-exp%C3%A9rimental-yellow)](#limitations-connues)

*Un fork de [`openai/codex`](https://github.com/openai/codex) qui comprend votre configuration Claude Code — sur place, sans migration.*

</div>

---

## Le concept

Vous avez des `CLAUDE.md`, des agents, des skills, des hooks et des commandes personnalisées pensés pour Claude Code, mais vous voulez tourner sur le moteur Codex avec votre abonnement ChatGPT ?

**Claudex lit votre configuration Claude Code existante là où elle se trouve.** Aucune copie, aucune migration, aucune clé API Anthropic — l'authentification ChatGPT officielle est le mode par défaut, et les quotas de votre compte s'appliquent.

| | |
|---|---|
| **Zéro migration** | `CLAUDE.md`, `CLAUDE.local.md`, `.claude/rules/` sont lus sur place |
| **Auth ChatGPT** | Authentification officielle Codex, aucune clé Anthropic |
| **Agents & rôles** | `.claude/agents/*.md` alimentent les rôles natifs (nom, description, instructions) |
| **Skills & commandes** | `.claude/skills`, `.claude/commands` — `/nom arguments` développe le Markdown dans le terminal |
| **Hooks** | Événements supportés routés vers le moteur de hooks natif, contexte projet/plugin fourni |
| **MCP** | `.mcp.json` et la configuration MCP de `.claude.json` (stdio et HTTP) |
| **Permissions** | `deny` / `ask` de `settings.json` appliqués avant le dispatch des outils |
| **Workflows** | Runtime JavaScript original : `agent`, `parallel`, `pipeline`, phases, pause/reprise/arrêt |
| **Mémoire** | Mémoire projet native avec hook et requête (`scripts/claude-memory*.mjs`) |

## La TUI

Un en-tête Claudex adaptatif (terminaux larges et étroits), un compositeur délimité et cinq entrées natives :

| Commande | Rôle |
|---|---|
| `/help` | Aide recherchable |
| `/agents` | Catalogue des rôles chargés (lecture seule) |
| `/tasks` | Sous-agents de la session en cours |
| `/workflows` | Exécutions locales, détails, pause / reprise / arrêt |
| `/agent-center` | Tableau multi-sessions, ouvrable depuis le terminal embarqué |

Les outils, sous-agents, sessions et extensions natifs de Codex restent intégralement présents.

## Utilisation

```powershell
claudex                                  # session interactive (TUI)
claudex login status                     # état d'authentification
claudex exec "Explique ce dépôt"         # exécution non interactive
claudex compat .                         # diagnostic de compatibilité du répertoire courant

claudex workflow film.workflow.js --args '@arguments.json' --run-id mon-film
claudex workflow list                    # exécutions locales
claudex workflow pause mon-film          # laisse finir l'agent actif, bloque les suivants
claudex workflow resume mon-film         # relance depuis les checkpoints
claudex workflow stop mon-film           # annule l'agent actif, conserve les checkpoints
```

Les workflows sont des programmes JavaScript de confiance exécutés par le binaire Claudex avec l'authentification officielle :

```js
// film.workflow.js
const synopsis = await agent("Rédige le synopsis", { phase: "écriture" });
const [visuel, voix] = await parallel([
  () => agent("Génère le storyboard", { phase: "production" }),
  () => agent("Écris le script voix off", { phase: "production" }),
]);
```

Reprise déterministe : après modification du script, le préfixe d'appels identiques est rejoué depuis le cache ; un agent échoué retourne `null` et n'est pas mis en cache. L'état des exécutions est conservé dans `~/.claudex/workflow-runs/<run-id>`.

## Installation

> **Plateforme :** Windows x64 (MSVC) pour l'instant.

Téléchargez `claudex-0.160.0-windows-x64.zip` depuis les [Releases](../../releases), décompressez-le et ajoutez le dossier au `PATH`. L'exécutable s'installe d'ordinaire dans `%LOCALAPPDATA%\Programs\Claudex\bin`.

Ouvrez un nouveau terminal puis lancez `claudex` et choisissez **Sign in with ChatGPT**.

## Construction depuis les sources

Prérequis : Rust 1.95.0 (host `x86_64-pc-windows-msvc`), MSVC + Windows SDK, Python 3.12, Node 22.

```powershell
git clone https://github.com/lyes117/claudex.git
cd claudex

powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-claudex.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/install-claudex.ps1

# Tests
node scripts/workflows.test.mjs
Push-Location codex-rs
just test -p codex-config -p codex-agent-roles -p codex-home -p codex-skills-extension -p codex-hooks -p codex-tui -p codex-cli --cargo-profile dev-small
Pop-Location
```

Le builder compile le CLI, CodeMode et les helpers Windows depuis les sources, résout la paire V8 sandbox publiée par OpenAI avec vérification de manifeste épinglé, et conserve les ressources voix/ripgrep du paquet officiel 0.160.0. Les mises à jour automatiques amont sont désactivées pour ne pas remplacer le fork par Codex standard.

## Architecture

Fork du dépôt `openai/codex` (moteur Rust), extensions Claudex réparties dans les crates existantes :

| Zone | Extensions |
|---|---|
| `codex-rs/core` | Contrôle des agents, runtime et persistance des workflows, plugins Claude |
| `codex-rs/tui` | En-tête Claudex, commandes `/agents` `/tasks` `/workflows` `/agent-center`, panneaux de plan |
| `codex-rs/config` | Expansion des commandes Claude, mémoire native, sélection de plugins |
| `codex-rs/hooks` | Arguments de commandes, construction des hooks |
| `codex-rs/cli` | Sous-commandes `workflow`, mémoire Claudex |
| `codex-rs/codex-api` | Routage `claudex_chat`, pont ZCode |
| `codex-rs/code-mode-*` | Hôte et protocole d'exécution des workflows |
| `scripts/` | Build Windows, mémoire Claude (hook, requête, proxy), tests Node |

La documentation détaillée du fork vit dans [`CLAUDEX.md`](CLAUDEX.md) ; les vérifications réellement exécutées sont consignées dans [`CLAUDEX-VERIFICATION.md`](CLAUDEX-VERIFICATION.md).

## Limitations connues

Claudex n'est **pas** une reproduction intégrale de Claude Code :

- Imports globaux `~` et activation dynamique complète des instructions non couverts
- `ask` des permissions devient un refus ; pas d'équivalence des règles de fichiers spécialisées
- Hooks `prompt`, `agent` et HTTP non exécutés ; shell Windows natif, pas de traduction Bash automatique
- MCP : SSE non supporté, tokens et OAuth Claude non importés
- Workflows : agents séquentiels (processus Codex distincts), pas de workflows imbriqués ; `node:vm` n'est pas un bac à sable de sécurité
- Le panneau `/workflows` ne lance pas encore de nouvelle exécution

La table de compatibilité complète, avec limites exactes par source, figure dans [`CLAUDEX.md`](CLAUDEX.md).

## Licence & crédits

- Moteur : [`openai/codex`](https://github.com/openai/codex), licence [Apache-2.0](LICENSE) conservée, notice [`NOTICE`](NOTICE) conservée
- Aucun code propriétaire de Claude Code n'est repris — les comportements sont réimplémentés depuis la configuration documentée
- Projet indépendant, **non affilié** à OpenAI ni à Anthropic
