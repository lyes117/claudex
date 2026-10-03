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
Les nombres flottants integres ne valent integer qu'en dessous de 2^53 en valeur
absolue ; les entiers JSON i64/u64 restent exacts. Ceci ne prouve pas la compatibilite
des schemas de film.workflow.js.

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
La revue trouve toutefois une course entre le retour de spawn et la capture par
lookup ID : un remplacement dans cet intervalle pourrait etre capture et arrete.
La capture de l'Arc au point d'admission natif reste necessaire avant activation.
Autre limite ouverte du validateur : la conversion f64 peut arrondir des petites
fractions precises en entier ou valeur enum. Le seuil 2^53 ne suffit pas ; il
faut comparer exactement les lexemes decimaux bornes ou refuser ce sous-ensemble.

La deadline couvre preparation et attente ; une admission deja commencee est
attendue avant arret, et la fermeture native n'a pas de deadline externe.
La livraison automatique des completions au parent reste celle de Codex et peut
preceder la validation locale. Elle doit etre adaptee avant activation publique,
avec revue du budget des fragments parents/enfants. Aucun outil Workflow n'est
enregistre et aucun runner JS n'est active par cette tranche. Les fixtures SSE
utilisent le runtime natif ; elles ne sont pas une inference ChatGPT ou un film reel.
