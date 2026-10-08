# Mémoire globale native : tranche en cours

Statut au 3 octobre 2026 : bundle construit et fixtures exécutées ; aucune
installation globale, capture réelle ou inférence observer encore vérifiée.
Le nouveau binaire installé contient désormais ce raccord CLI ; son status
indique correctement `installed:false`, service non configuré.

## Sources et installation

La source publique claude-mem est épinglée à
`a1951f2ad247330b2b5d58a1e0c7efeef4a03be5` et conservée sous
`.build-tools/claude-mem-upstream`. Le bundle construit est
`.build-tools/cm-build-a1951f2-cx2-cycle1/package`, édition explicitement marquée
`13.29.0-dev+a1951f2.cx2`, licence Apache-2.0. La release npm 13.28.0 inspectée ne
contient pas son fournisseur Codex ; elle ne constitue pas un remplacement
acceptable. Les locks, empreintes et receipts du build restent dans .build-tools.

Le helper installe seulement le marketplace dédié `claudex-memory` et le plugin
canonique `claude-mem@claudex-memory`. Les données nouvelles sont séparées sous
`~/.claudex/memory/data`, port 37778. Les fichiers, base et réglages Claude-mem
existants sont préservés ; leur historique n'est pas importé ou fédéré.
Les dépôts ne reçoivent ni migration de leur configuration ni fichier AGENTS
généré pour cette intégration.

## Observer et appels natifs

Le fournisseur utilise l'app-server du binaire Claudex officiellement
authentifié par Codex/ChatGPT. Il ne requiert pas une clé Anthropic ou OpenAI
API séparée. Son processus app-server possède HOME/USERPROFILE privés ; son
chemin d'authentification est résolu avant cette isolation et lié selon le
mécanisme amont, sans copie du contenu. Outils et hooks sont désactivés
explicitement côté observer. Les watchers de transcripts et la synchronisation
cloud sont désactivés dans cette tranche ; concurrence observer limitée à un.
L'attestation et l'absence réelle d'outils restent à vérifier sur le protocole
en fonctionnement, pas seulement dans une fixture.

Les hooks natifs transmettent les événements SessionStart, UserPromptSubmit,
PreToolUse, PostToolUse et Stop au bundle épinglé. Le worker conserve son cwd
d'installation ; le serveur MCP conserve le cwd du projet appelant. Les helpers
vérifient les empreintes du bundle et l'identité du worker avant leurs appels.
Un service existant sur un autre état/port ne doit pas être arrêté ou réutilisé.

## Sélection du plugin

Le marqueur `native-active.json` exprime l'intention d'activation. Son format
est strict et borné à 1024 octets. Il ne prouve pas le chargement du plugin.
Avant exclusion des capacités du plugin legacy, la sélection doit aussi
vérifier le résultat effectif de PluginsManager, les exclusions du thread,
`LoadedPlugin::is_active()` et l'autorisation de configuration utilisateur.

La provenance d'une contribution MCP legacy doit être conservée jusqu'à cette
sélection : retirer simplement un nom de serveur pourrait retirer une
définition explicite du dépôt. Les racines hooks/skills et leur cache doivent
suivre la même sélection, y compris le préwarm. Le plugin natif absent, refusé
ou désactivé doit préserver les contributions legacy autorisées.

## Vérifications et étapes restantes

Les 34 tests Node des helpers, identités, requêtes et refus du reclaim passent dans
`tests-claudex-memory-cx2-combined-cycle1.log`, sans skip. La préparation refuse un root
existant sans reçu et crée le nouveau root exclusivement avant ses écritures.
Une préparation interrompue est conservée et refusée à la reprise. Les contrôles
start/stop vérifient la santé après une sortie zéro du helper ; stop vérifie
aussi que le port est libre. Les budgets hooks par action restent ceux du bundle
amont (20/20/30/120/60 secondes). Les fixtures source du fournisseur
donnent 53 réussites, 16 skips Windows et zéro échec ; aucune inférence réelle
n'est comprise dans ces nombres. Le build ignore les scripts lifecycle des
dépendances et vérifie les locks lors d'un second passage frozen.

Avant livraison : revue adversariale des helpers, sélection effective et tests
des collisions MCP, build du CLI, installation ciblée, puis auth et inférence
observer, capture/injection et recherche MCP dans deux projets synthétiques.
Contrôler les paramètres observer sur le protocole et l'absence de changement
des fichiers Claude-mem/configurations sources.

Les onze tests ciblés de sélection/configuration passent, ainsi que les
nouveaux cas core/hooks/skills dans la campagne de quatre bibliothèques et
le cas d'invalidation du watcher app-server dans la reprise ciblée.
La sélection vérifie le chargement natif effectif, les contributions MCP
explicites sont conservées et les caches/catalogues utilisent la même autorité.
Ces fixtures ne prouvent pas une transition réelle du marqueur natif pendant
une session. La création future d'un parent de marqueur absent n'est pas surveillée.

Le schéma `cx1-sha256-canonical-root` remplace les basenames : racine physique
canonique, identité SHA-256 complète, liens explicites parent/worktree/submodule.
Douze tests utilisent de vrais dépôts Git et jonctions Windows. Le même resolver
est embarqué dans le bundle pour hooks, MCP, remap et CLI ; son hash et le schéma
font partie des reçus obligatoires. L'ancien bundle sans cx1 est refusé.
Déplacer un dépôt crée une nouvelle identité, sans migration automatique.
La normalisation Windows des chemins sensibles à la casse et les noms POSIX
contenant un retour à la ligne restent des limites.

Les commandes natives `memory search` et `memory context` renvoient désormais
les contenus HTTP du worker, dans la portée du cwd par défaut. Leur fixture
emploie un serveur HTTP synthétique ; elle ne prouve pas une recherche mémoire
réelle. La lecture du corps est bornée avant parsing. Cinq tests Clap passent,
dont les refus des options runtime inapplicables et la prise en compte de `-C`.
Le binaire installé expose ces commandes. Leur status, l'authentification
ChatGPT et les refus d'options runtime sont vérifiés en PowerShell ; lancement
et status également en CMD. Les opérations avec une mémoire réelle restent à vérifier.

L'édition cx2 refuse tout reclaim automatique de port : le module épinglé
retourne explicitement un refus sans découvrir ni tuer un PID. Le nouveau
motif appartient au contrat TypeScript. Une fixture Bun appelle le vrai module
patché avec des dépendances interdisant tout accès ; le receipt exige cette
politique. Le bundle cx2 est construit et ses hashes revérifiés ; cela ne
constitue pas une preuve du lifecycle réel ou de toutes les opérations amont.

Un proxy d'instrumentation transparent et un launcher Windows avec Job Object
ont 15 premiers tests synthétiques réussis. La revue trouve des raccords manquants
(sidecar traversant le filtrage d'environnement, checkpoint avant kill et
association des captures aux audits) ; leurs corrections sont en cours. Ils
n'ont encore observé aucune inférence réelle. Aucun contenu de protocole brut
ne doit être journalisé. Les helpers désactivent explicitement la télémétrie.

Limites à traiter : matcher PreToolUse ne couvrant pas tous les outils Read/Grep/Glob, redaction
non exhaustive, garde de port préalable non atomique et commande de désinstallation
absente. Arrêter le service ne désactive pas le plugin. La santé du worker et le
chargement du plugin sont deux preuves distinctes.

Source primaire : [fournisseur Codex claude-mem](https://github.com/thedotmack/claude-mem/blob/a1951f2ad247330b2b5d58a1e0c7efeef4a03be5/docs/codex-provider.md).
