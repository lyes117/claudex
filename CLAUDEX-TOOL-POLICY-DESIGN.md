# Restrictions natives des sous-agents : conception verifiee

Etat : heritage parental a chaud implemente, revu et installe avec les helpers compiles depuis les sources ; restauration durable et contraintes des profils Claude encore ouvertes. Les profils qui portent des contraintes non prises en charge restent refuses. Les controles d'exposition et de dispatch de plafonds utilisent des sessions natives de fixture, pas une inference ChatGPT sous profil Claude contraint.

## Contrat de compatibilite

La [reference officielle des sous-agents Claude](https://code.claude.com/docs/en/sub-agents), consultee le 3 octobre 2026, distingue `tools` et `disallowedTools`. Une liste autorisee absente herite des outils ; les refus retirent des outils de cet ensemble. Les selecteurs MCP peuvent viser un serveur. Les transports CodeMode de Codex ne sont pas des outils Bash. Leurs noms ne doivent donc pas etre assimiles aux aliases de commandes shell.

Ce sont des contraintes d'acces aux outils. Elles ne prouvent pas une isolation generale du systeme de fichiers : un shell autorise ou un outil composite peut disposer de plusieurs capacites. Les modeles Anthropic restent hors du moteur ChatGPT demande.

## Points natifs et lacunes de l'audit initial

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

## Verification du premier etage

Les controles cibles passent : exposition et dispatch directs d'un enfant avec allowlist parentale vide ou limitee, exclusion des appels imbriques CodeMode avec execution d'un outil permis, delegation `/review`, immutabilite des captures malgre remplacement des attachments, reprise active refusee sous un plafond plus etroit et fork conservant le plafond de sa source immediate. Les chemins de fork Legacy, copie d'une source Paginated et Prepared sont controles pour cette autorite ; la copie Paginated de cette fixture utilise une histoire vide et ne prouve pas la copie de contenu.

Deux courses deterministes verifient le fallback sender V2 : une publication compatible conserve le runtime concurrent ; une publication trop large est refusee. Le candidat provient volontairement d'un autre manager isole et est injecte sous le vrai lock apres une barriere de startup. Cela prouve le fallback face a la collision de publication, pas deux reloads ordinaires concurrents, leur acquisition de writer ni leur ownership inter-manager.

Les tests utilisent des sessions natives et des reponses de modele de fixture, sans inference ChatGPT ni workflow marketing de production. L'API d'extensions passe 12/12 tests, dont 1600 compositions de plafonds. La suite core complete donne 4134 reussites, 111 echecs, 2 expirations et 77 ignores ; elle n'est pas verte. Les journaux `tests-hot-policy-core-cycle7.log`, `tests-hot-policy-core-races-cycle8.log`, `tests-hot-policy-api-all-cycle1.log` et `tests-hot-policy-core-all-cycle1.log` conservent ces limites.

Apres construction du vrai serveur MCP de fixture et du helper CodeMode depuis les sources, une execution sous HOME/USERPROFILE isoles donne 147 reussites sur 149, un echec CLI et une expiration MCP. La matrice MCP de 64 combinaisons passe ensuite en 58,620 secondes avec deux workers et une limite bornee a 180 secondes. Huit des neuf fixtures CLI passent dans ce second cycle ; la fixture restante de creation/reprise passe seule apres correction de ses overrides mock. Ces reprises ciblees ne constituent pas une nouvelle suite complete verte. Journaux : `tests-hot-policy-source-helper-isolated-cycle1.log`, `tests-hot-policy-cli-mcp-isolated-cycle2.log`, `tests-hot-policy-cli-resume-mock-cycle4.log`.

Le defaut ChatGPT de production reste dans `config/defaults.toml`. Les fixtures CLI API declarent explicitement leur mode mock ; aucune configuration ou authentification personnelle n'est modifiee. La fixture de reprise conserve les overrides globaux avant son sous-commande et borne aussi son endpoint par OPENAI_BASE_URL vers le serveur local. Elle ne prouve pas que des occurrences de `-c` reparties de part et d'autre d'une sous-commande se cumulent correctement.

## Revue du prochain etage durable

La revue adversariale trouve une limite distincte de la restauration : ResumeThreadParams et RolloutRecorderParams::Resume ne remplacent pas le premier header canonique. Restaurer un plafond de creation, puis composer une nouvelle reduction sans la persister, permettrait de perdre cette reduction au redemarrage suivant. Ajouter une nouvelle SessionMeta en fin de fichier ne suffit pas, puisque la premiere reste autoritaire.

La premiere tranche durable doit annoncer seulement un plafond de creation immuable. Tant qu'une rotation canonique ou un journal monotone sous ownership n'est pas verifie, elle doit refuser une reprise persistante dont le resultat est strictement plus etroit que le snapshot enregistre, et refuser une nouvelle policy restrictive sur un legacy sans snapshot. L'equivalence se teste par is_subset_of dans les deux sens. Un Guardian neuf peut enregistrer son plafond ; un Guardian legacy restreint sans snapshot doit etre refuse explicitement.

La restauration doit verifier SessionMeta.id contre le thread repris ou la source immediate du fork, jamais session_id (racine) ni history_base.thread_id (source physique). Le DTO versionne et borne doit vivre dans protocol sans dependance extension-api. Tous les lecteurs qui ignorent les lignes mal decodees doivent valider le vrai header avant de pouvoir retenir une metadata ancetre. Format strict, capture/restauration et preuves natives forment trois tranches distinctes ; aucune n'est encore implementee. La provenance des outils et les selecteurs MCP restent indispensables avant d'activer les profils Claude contraints.

### Format strict verifie, sans activation runtime

`protocol/src/tool_policy_snapshot.rs` implemente le DTO version 1 : 8 Kio bruts avant decodage, 128 outils, composants de 256 octets maximum sans caracteres de controle. Le constructeur controle aussi la taille JSON canonique. Tous les champs sont obligatoires, y compris les nullable `allowed_tools` et `namespace`. La liste vide reste distincte de null. Champs inconnus, doublons JSON, versions inconnues et identites dupliquees sont refuses ; les aliases du namespace functions sont compares selon leur semantique native. Les identites restent des tuples, sans concatenation ambigue.

Le DTO expose Serialize mais pas Deserialize general : les octets persistants doivent passer par `from_json_slice` avant toute conversion en Value. Le plafond valide reste immuable. La suite protocole passe 358/358 tests, dont six nouvelles matrices adversariales (`tests-tool-policy-snapshot-protocol-cycle1.log`) ; Clippy cible et formatage passent. La revue ne trouve aucun P0/P1/P2 dans cette tranche. Le DTO n'est pas encore raccorde a SessionMeta ou a la restauration ; les trois autres garanties, header canonique, capture native et reprise froide, restent a implementer et verifier.

### Garde des lecteurs de header verifie, sans restauration runtime

`rollout/src/tool_policy_header.rs` lit le payload avec RawValue emprunte, valide le plafond avant tout passage par Value, puis valide aussi la metadata ordinaire. Les trois chemins `load_rollout_items`, `read_head_summary`, `read_session_meta_line` et le lecteur `read_head_for_summary` invoquent ce garde avant de pouvoir ignorer un premier header indecodable. Type absent/null, policy explicite hors session_meta, doublons des cles d'autorite, policy null/inconnue/invalide et JSON malforme avant header echouent explicitement. Les types JSON echappes restent acceptes. Le garde ne traite pas une metadata ancetre tardive comme l'autorite du rollout.

La revue a reproduit puis fait corriger un repli via type absent/null et le refus indu d'un type echappe. La suite rollout finale passe 138/138, avec controles positifs legacy et negatifs confrontes a un ancetre valide (`tests-tool-policy-header-rollout-cycle4.log`). Les cycles 1 a 3 conservent les echecs intermediaires ; formatage et Clippy cibles passent. `just bazel-lock-update` passe ; la feature raw_value etait deja unifiee, donc aucun diff du lock n'est produit.

Le contrat de compatibilite change volontairement : du JSON malforme AVANT le premier header est refuse, tandis que la tolerance des lignes malformees tardives reste controlee. Le garde compare deux decodages de l'ID canonique, pas encore l'ID source attendu. Ordinal/projection/migration restent hors de cette tranche ; l'autorite doit toujours etre validee par le chemin de restauration avant activation. Aucun plafond n'est encore enregistre ni restaure par le runtime installe.
