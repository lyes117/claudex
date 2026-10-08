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
pas une inference ChatGPT ni un ordonnanceur Workflow. Le code de transport est
compile et installe avec f8d7543 ; les hashes source/installation sont verifies
dans installed-no-tools-and-schema-native-hashes-cycle1.json. La validation et le budget du schema restent a ajouter au
service Workflow futur avant son activation.

## Tranche locale en cours de validation

`agent/control/workflow.rs` capture la session, le step et l'annulation depuis
une invocation native. Le groupe entier est prepare avant admission ; seules
les identites retournees par les admissions sont fermees. Le superviseur attend
toutes les completions, preserve l'ordre des appels et attend les fermetures.
Les enfants frais ont la delegation desactivee dans cette tranche.

Limites : quatre enfants, prompt et schema de 8 KiB chacun, resultat du groupe
de 8 KiB ; JSON de profondeur 12 et 256 noeuds. Le validateur refuse les mots-cles
inconnus, les doubles cles resultat, les schemas ouverts et les resultats invalides.
Son sous-ensemble comprend type, properties, required, additionalProperties=false,
items, enum et description ; pas de ref, union, format, pattern ou bornes numeriques.
Les nombres sont compares comme decimaux exacts, sans conversion f64 ni expansion
des exposants. Le lexeme est borne a 8192 octets et l'exposant explicite a +/-8192.
Les representations 2, 2.0 et 2e0 sont equivalentes ; une fraction precise ne peut
pas devenir un entier par arrondi. Ceci ne prouve pas la compatibilite des schemas
de film.workflow.js.

Avant admission, le bridge applique aussi les contraintes de transport documentees :
racine objet, toutes les proprietes requises, objets fermes, y compris dans les items
imbriques. Source : [Structured Outputs officiel](https://developers.openai.com/api/docs/guides/structured-outputs).
Ce refus n'adapte pas les schemas Claude optionnels. Le decoder distingue les
conteneurs reels des nombres arbitrary_precision et refuse les doubles cles.
Les flottants integres suivent [la semantique integer de JSON Schema](https://json-schema.org/understanding-json-schema/reference/numeric)
dans la plage prise en charge.

La fermeture utilise les Arc des threads possedes et attend leur terminaison avant
de retirer un runtime encore identique. Elle ne depend pas d'une barriere de
persistance reussie avant l'arret. Une reprise remplacant le runtime ne doit pas
etre retiree par un cleanup tardif ; le statut attendu reste celui du thread capture.
Le bridge capture maintenant l'Arc dans l'admission native ; l'envoi initial,
le statut et le cleanup utilisent cette meme instance. Un guard reste arme
jusqu'au handoff final et ne retire un runtime que si l'Arc est encore identique.
Le cleanup ne ferme pas le writer d'un remplacement apres l'arret du thread
original. Les dix fixtures couvrent aussi une reprise sur le meme store, un
thread retire par InternalAgentDied et la fermeture de son edge SQLite.

La campagne ciblee `tests-claudex-native-ownership-numeric-cycle3.log` passe
110/110 (57,789 s), incluant les dix fixtures de propriete et treize cas de
schema/decimaux. Le cycle1 n'avait execute aucun test (erreur de compilation
de fixture). Le cycle2 passait 109/110 : sa fixture de guard utilisait trois
registres independants ; elle utilise maintenant le meme AgentControl et
conserve les assertions de registre avant et apres Drop. Ce resultat valide
les mecanismes testes, pas l'activation d'un outil Workflow.

La deadline couvre preparation et attente ; une admission deja commencee est
attendue avant arret, et la fermeture native n'a pas de deadline externe.
La livraison automatique des completions au parent est maintenant controlee par
une politique privee `CompletionReporting`, capturee de facon immuable avant
startup. `SupervisorOwned` supprime les notifications brutes legacy/V2 ; les
threads ordinaires conservent `Automatic`. La mutation ulterieure des extensions
ne change pas cette politique. Une admission warm de propriete differente est
refusee avant mutation ; le fallback V2 preserve aussi ce refus (revue statique).

Les 17 fixtures de reporting passent dans
`tests-claudex-recovery-cli-memory-tui-cycle1.log` : V1/V2, User/AgentMessage,
resultats invalides/oversize, refus host factory, warm reuse et FullHistory fork.
Le statut final, les resultats et l'absence de notification brute sont verifies
par les fixtures natives SSE, sans inference reelle. Le restore froid de cette
politique et le budget des fragments de resultat restent a traiter.

Aucun outil Workflow n'est
enregistre et aucun runner JS n'est active par cette tranche. Les fixtures SSE
utilisent le runtime natif ; elles ne sont pas une inference ChatGPT ou un film reel.
