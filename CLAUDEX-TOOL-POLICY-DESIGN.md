# Restrictions natives des sous-agents : conception verifiee

Etat : audit des sources et revue adversariale effectues ; aucune restriction Claude supplementaire implementee par ce document. Les profils qui portent des contraintes non prises en charge restent refuses.

## Contrat de compatibilite

La [reference officielle des sous-agents Claude](https://code.claude.com/docs/en/sub-agents), consultee le 3 octobre 2026, distingue `tools` et `disallowedTools`. Une liste autorisee absente herite des outils ; les refus retirent des outils de cet ensemble. Les selecteurs MCP peuvent viser un serveur. Les transports CodeMode de Codex ne sont pas des outils Bash. Leurs noms ne doivent donc pas etre assimiles aux aliases de commandes shell.

Ce sont des contraintes d'acces aux outils. Elles ne prouvent pas une isolation generale du systeme de fichiers : un shell autorise ou un outil composite peut disposer de plusieurs capacites. Les modeles Anthropic restent hors du moteur ChatGPT demande.

## Points natifs et lacunes constatees

- `ext/extension-api/src/tool_policy.rs` : plafond fourni avant le demarrage par `ExtensionDataInit`, avec allowlist exacte et exigences sandbox/unified-exec. Le contrat exige actuellement que l'appelant le fournisse de nouveau a la reprise.
- `core/src/session/session.rs` : capture du plafond dans un `Arc<ToolPolicy>` immuable pour le runtime.
- `core/src/tools/registry.rs` et construction des specs : filtrage des outils avant leur exposition et dispatch.
- `core/src/agent/control/spawn.rs` : les chemins de creation, fork et reprise ne transmettent pas tous le plafond du parent. Le fork fournit essentiellement les capability roots.
- `protocol` : aucun plafond d'outils dans les snapshots actuellement utilises pour restaurer une session.
- `core/src/tools/spec_plan.rs` : une Function cliente `Read` peut remplacer le Read natif ; les outils clients sont aussi restaures depuis l'historique. Un simple nom ne suffit donc pas a accorder les garanties du Read natif.

## Implementation attendue

Conserver les fichiers `.claude/agents` comme sources. Parser leurs contraintes dans une structure typee, distincte du prompt. Composer les restrictions du role avec le plafond deja capture du parent : conjonction des allowlists, union des denies, exigences sandbox/unified-exec par OR et exposition des permissions additionnelles par AND. Une liste vide ne doit pas devenir une absence de restriction.

Conserver des clauses immuables pour les denies et selecteurs MCP ; un inventaire ponctuel des outils presents ne couvre pas les outils decouverts plus tard. Resoudre MCP avec les metadonnees originales de serveur et outil, ainsi que le nom canonique natif. Ne pas utiliser `ToolName::Display` comme identite : il concatene les composants sans delimiteur.

Pour un alias Claude accordant un outil natif, distinguer la provenance d'une Function cliente homonyme. Refuser une collision incompatible dans un thread restreint ; garder la priorite client existante dans les threads non restreints.

Transmettre le plafond compose dans `ExtensionDataInit` pour chaque creation et chaque fork/reprise. Enregistrer un snapshot normalise dans le `SessionMeta` canonique deja persiste. Le restaurer avant la capture du plafond et avant la construction des outils/MCP. A la reprise, combiner le snapshot avec le plafond parental actuel et la restriction actuelle du role ; un role supprime ou invalide ne doit pas produire silencieusement un thread unrestricted.

Le lecteur d'historique ignore actuellement les records indecodables. Une politique invalide ou une version inconnue dans le vrai header doit echouer explicitement, sans retomber sur une metadata ancetre du fork. Verifier aussi que l'identite de cette metadata correspond au thread repris. Les anciens historiques sans contrainte doivent conserver leur compatibilite sans fabriquer un plafond historique inexistant.

## Preuves requises avant activation

1. Un descendant ne peut pas accorder davantage que son parent.
2. Un role elargi, supprime ou invalide apres l'arret n'elargit pas la reprise.
3. La politique est restauree en Legacy et Paginated, y compris dans les forks.
4. Un outil MCP ajoute tardivement reste soumis aux denies et selecteurs de serveur.
5. Une Function cliente `Read`, ou une imitation de nom MCP, n'obtient pas les garanties d'un outil natif autorise.
6. Le broker CodeMode refuse effectivement un outil interdit ; les transports ne relachent pas le plafond parental.
7. Une politique persistee malformee ou inconnue echoue explicitement.
8. Les snapshots et clauses ont des tailles bornees ; aucun payload d'outil ou secret d'authentification n'est persiste dans le plafond.

L'outil conversationnel Workflow et le gestionnaire de profils doivent reutiliser ce mecanisme, une fois ses invariants verifies. Une copie de configuration Claude, un prompt disant de respecter une restriction, ou des processus enfants lances sans ce plafond ne constituent pas cette integration.

## Premier etage : heritage parental a chaud

La revue des chemins de construction retient `ThreadManagerState::spawn_thread` comme point commun de composition, avant l'initialisation MCP et la capture dans `Session`. Capturer auparavant un `Arc<ToolPolicy>` dans les chemins qui possedent deja le parent, notamment `InternalSessionParent`, les creations AgentControl et les forks racines. Une eviction du parent pendant la preparation ne doit pas effacer ce plafond.

Deux controles sont indispensables :

- Resoudre la politique locale effective avant composition, y compris le fallback des reviewers Guardian. Inserer directement la politique parentale dans une init vide court-circuiterait ce fallback.
- Composer avant le retour anticipe d'une reprise active. Si la politique capturee du runtime existant depasse le plafond requis, refuser la reprise au lieu de retourner ce runtime ou de pretendre modifier son plafond immuable.

Pour un fork racine, la source immediate fournit l'autorite, et non son parent historique. Les tests de fork dont la source est dechargee pendant la preparation, de parent inline hors registre, de reprise active et de visibilite de l'init par MCP/lifecycle offrent les fixtures natives a etendre. Ajouter les chemins frais, full-fork, last-N et reprise arretee avec parent resident, en verifiant l'exposition et le dispatch effectifs, dont CodeMode.

Cet etage ne suffit pas a un parent froid ou absent : tant que la restauration durable n'existe pas, aucune autorite parentale ne peut etre reconstruite dans ce cas. Les contraintes des profils Claude restent refusees jusqu'aux preuves du stage durable.
