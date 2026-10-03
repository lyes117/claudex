# Workflows natifs : contrat et tranches

Statut : architecture revue, transport initial du schema valide par quatre fixtures natives, aucun outil Workflow actif. Le runner installe
`scripts/workflows.mjs` utilise encore des processus `claudex exec` distincts.
Le moteur de `film.workflow.js` a ete retrouve et inspecte sans execution dans
`Desktop/Studyshare/marketing/marketing studyshare/moteur/creation`.

## Autorite et integration

Un service Rust capture `ToolInvocation.session`, `step` et l'annulation. Le JS
ne peut choisir ni parent, ni environnement, ni politique. La configuration
enfant passe par `prepare_agent_spawn_config`, puis `AgentControl::spawn` avec
les bindings exacts et la provenance native. Les labels metier ne conferent
aucun role ou droit. Un CLI autonome doit creer son propre parent natif.

Le trait `AgentControl` ne fournit pas actuellement une attente et fermeture
generiques. La premiere integration peut donc annoncer explicitement un backend
local et utiliser son abonnement de statut et sa fermeture attendue. Attendre
le premier enfant termine ne suffit pas pour un groupe. Une interruption ne
prouve pas que les ressources sont fermees.

## Tranches independantes, moins de 500 lignes chacune

1. Transporter le schema optionnel dans `SpawnAgentOptions`, jusque dans le
   `TurnStartOptions` initial. Prouver sa presence dans la premiere requete
   enfant et conserver le comportement sans schema. Aucun Workflow active.
2. Service local d'appel enfant : config capturee, provenance, attente, resultat
   borne, validation du schema et fermeture des seules ressources possedees.
3. Runner JS avec limite memoire effectivement appliquee et deadline externe,
   interruption des boucles CPU et destruction fiable. Le champ heap du runtime
   actuel est ignore par `InProcessCodeModeSession`; il n'est pas une garantie.
4. DSL limite avec DTO bornes ; aucun objet Node, outil ordinaire ou handle hote.
   Les imports sont rejetes avant toute admission d'enfant, meme places apres
   un premier appel. `node:vm` ne constitue pas une isolation de securite.
5. Journal hote, groupes bornes, ordre des resultats, rejeu du prefixe, arret et
   reprise. Refuser une reprise sous une autorite absente ou differente tant
   que la restauration durable du plafond n'est pas implementee.
6. CLI et panneau TUI relies au service valide, controles et progression reels.

La livraison intermediaire des completions au parent doit etre une politique
explicite. Masquer seulement les cartes TUI ne retire pas ces messages du
contexte. Le rejeu ne garantit pas l'unicite des effets : un crash apres un effet
et avant checkpoint peut le repeter. La concurrence ne protege pas les fichiers
marketing partages contre des ecritures concurrentes.

## Preuves adverses requises

Plafond parental direct et CodeMode ; config enfant permissive ; parent et
chemin de script falsifies ; annulation pendant preparation, spawn et inference ;
JSON invalide ou volumineux ; schema contradictoire ; import tardif ; boucle CPU,
allocation excessive et boucle apres await ; reprise avec enfants encore actifs.
Les fixtures prouvent les mecanismes testes, pas des effets marketing reels.

Sources : code natif `core/src/agent/{types.rs,child_config.rs,api.rs}` et
`core/src/agent/control/{spawn.rs,watch.rs,completion.rs,legacy.rs}` ;
[contrat public Workflow](https://code.claude.com/docs/en/workflows).
Aucun moteur proprietaire Claude n'est copie.

La suite core/config cycle2 a ete compilee avant la tranche schema ; elle valide
le plafond tools.enabled precedent. Les quatre cas schema sont executes
separement dans le cycle2 ci-dessous ; ils ne prouvent pas un Workflow complet.

`tests-native-initial-schema-cycle1.log` : compilation de la fixture refusee
car SpawnAgentOptions attend un snapshot d'environnement, pas une Vec. Aucun
test execute. La fixture est corrigee avec un snapshot vide explicite ; la
configuration de startup parent conserve son type distinct.

`tests-native-initial-schema-cycle2.log` : quatre tests executes, quatre
reussites (4,575 s apres compilation). Le schema est present des la premiere
requete enfant pour UserInput et AgentMessage ; les deux controles sans schema
restent inchanges. Ce sont des fixtures SSE avec le runtime natif et sa provenance,
pas une inference ChatGPT ni un ordonnanceur Workflow. Build/install non executes
pour cette tranche. La validation et le budget du schema restent a ajouter au
service Workflow futur avant son activation.
