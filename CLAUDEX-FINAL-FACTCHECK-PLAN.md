# Audit final de la fusion Claude Code + Codex + Z.ai

Demandé par l'utilisateur le 4 octobre 2026, pendant la reprise après redémarrage.
Cet audit doit être exécuté après la réalisation de la fusion et avant toute
déclaration de parité complète. Ce document fixe le périmètre ; il ne constitue
pas un résultat d'audit et ne valide aucune capacité par sa seule présence.

## Périmètre à conserver

L'objectif reste l'ensemble de l'écosystème Claude Code reproduit dans Claudex,
avec les capacités supplémentaires de Codex préservées et un routage intelligent
vers les modèles Z.ai lorsque la tâche et le quota le justifient. Un CLI qui
charge seulement les formats Claude ou passe une fixture n'est pas la preuve de
cet objectif. Les écarts trouvés doivent retourner au chantier d'implémentation.

| Domaine | Contrôles à détailler dans l'inventaire final |
| --- | --- |
| TUI | Accueil, rendu live, transcript, composer, menus, états, raccourcis, paste Unicode, resize, terminal Windows réel, annulation et erreurs |
| Prompts et contexte | Prompts réellement chargés, sources et priorités, orchestration, budget, frontières de cache, instructions de projet, reprises et compaction ; distinguer texte exact, contrat observable et comportement mesuré |
| Outils | Catalogue et schemas, sélection, dispatch, résultats, erreurs, annulation, Read/Write/Edit/Glob/Grep/Bash et outils Codex, outils directs et CodeMode |
| Skills et commandes | Découverte globale/projet/plugin, métadonnées, invocation naturelle/explicite, arguments, contexte, permissions et hooks associés |
| Agents et tâches | Rôles, sélection modèle/fournisseur, délégation native, contexte, communication, concurrence, tâches partagées persistantes, arrêt, résultats et navigation |
| Workflows | Déclenchement conversationnel, JS réel, phases, groupes parallèles, schémas, enfants natifs, journal, progrès TUI, pause/arrêt, reprise, modifications du script, crash et workflows imbriqués |
| Hooks | Événements, argv, payloads, matching, pré/post-exécution, édition multi-fichiers, refus, mises à jour d'entrée, trust, délais et shell Windows |
| Permissions et configuration | Priorités utilisateur/projet/gérées, règles, plafonds effectifs, environnements, reprise et fork ; aucun contournement par un changement de modèle |
| MCP et plugins | Transports, découverte, activation/désactivation, outils et ressources, OAuth, configuration en place, caches, changements pendant une session et provenance |
| Sessions et mémoire | Création, fork, reprise froide, historique, checkpoints, absence de doublons, mémoire native, capture/injection/recherche réelle dans plusieurs projets et préservation de l'existant |
| Codex conservé | Authentification officielle ChatGPT, extensions, modèles/capacités, fichiers et outils multimodaux, navigation/contrôle du navigateur et du PC, annulation et reprise des outils |
| Routage Z.ai | Off/auto/demande explicite, modèles réellement disponibles, capacités requises, critères de choix, abonnement Coding Plan, quota, budget, admission, concurrence, erreurs, circuit, retour vers Codex et reprise sûre |
| Livraison Windows | Build reproductible, hashes sources/bundle/binaire installé, PowerShell/CMD, PATH, helpers, terminal réel, préservation des dépôts et installations existantes |

## Preuve exigée pour chaque ligne atomique

L'inventaire doit partir des contrats et comportements de référence actuels,
pas uniquement des fichiers déjà présents dans Claudex. Les sources officielles
et les clones publics locaux doivent être datés et leurs versions consignées.
L'inspection du code actuel doit identifier le chemin exécuté, ses callers et
les restrictions effectives. Une fonction sans caller de production ne suffit pas.

Chaque ligne du rapport contiendra :

- Identifiant stable, comportement attendu et scénario concret.
- Source de référence, version/date et emplacement précis.
- Implémentation Claudex et conditions d'activation/configuration.
- Preuve du binaire réellement installé, procédure et résultat observé.
- Cas nominal, refus, erreur, annulation et reprise pertinents.
- Statut `complet`, `partiel`, `absent`, `non vérifiable` ou `régression`.
- Gravité, limites et action nécessaire pour clôturer l'écart.

Les preuves statiques, fixtures synthétiques, tests natifs et essais réels doivent
être distingués. Aucun pourcentage global ni résultat vert ne remplacera les
preuves par comportement. Les données sensibles ne doivent pas apparaître dans
les captures, transcripts, rapports ou paramètres des commandes de vérification.

## Prompts système

Vérifier les prompts effectivement utilisés par Claudex et les contrats publics
de composition de Claude Code. Pour une exigence de reproduction exacte du texte,
il faut une source accessible et une comparaison justifiable. Un prompt interne
non accessible reste `non vérifiable`, avec son effet attendu et les évaluations
comportementales possibles explicités. Ni un prompt de plugin publié, ni une
interface de composition ne prouvent le contenu du prompt système principal.
Ne pas présenter une couche d'orchestration originale comme une copie exacte.

## Routage intelligent et quota Z.ai

Les critères du routeur doivent intégrer type/risque de tâche, outils et modalités
nécessaires, contexte admissible, modèle demandé, qualité mesurée, quotas connus,
latence et état du fournisseur. Ne pas déclarer le routeur intelligent sur la
seule base de mots clés, de la longueur du prompt ou d'un changement de modèle.

Les tests doivent montrer un parent Codex et un enfant Z.ai distincts, les décisions
et leurs raisons, une authentification dédiée, l'absence de transfert de secrets,
le maintien des permissions et les refus de capacités incompatibles. Mesurer le
quota réellement observable : tokens rapportés et points de Coding Plan ne sont
pas assimilés sans preuve. Tester quotas inconnus/épuisés, timeout, annulation,
erreur d'authentification et effet déjà possible. Aucun fallback silencieux vers
une facturation API ni répétition automatique d'un effet externe.

## Contrôle du PC et autres capacités Codex

L'audit doit inventorier les capacités Codex de la version cible et vérifier leur
conservation dans le fork, sans se limiter aux outils actuellement exposés à un
agent de développement. Identifier dépendances, providers, catalogue, activation
et restrictions de la session. La présence d'un module, d'un plugin ou d'un
connecteur ne prouve pas l'action sur un vrai navigateur ou un vrai PC. Les essais
réels doivent être autorisés, bornés et sans modification de données hors périmètre.

## Clôture

Confronter les exigences initiales, toutes leurs extensions demandées et cet
inventaire aux preuves. Revue adverse indépendante des résultats, cas négatifs
et frontières de validation. Tout écart réalisable reste du travail ouvert ; une
capacité inaccessible doit être nommée précisément. Le rapport doit présenter
séparément version installée, sources préparées et travaux encore non activés.
Ne déclarer la fusion complète qu'avec des preuves couvrant le périmètre complet.

## Précisions du 5 octobre

L'authentification fait partie du périmètre : sign-in Z.ai depuis Claudex, comme
ZCode, avec cycle connexion/annulation/expiration/refresh/logout et stockage
séparé protégé. Ne pas présenter une connexion ZCode existante, une copie de ses
credentials ou un écran factice comme un login Claudex. Vérifier le flow effectif
et le type de credential utilisé pour l'inférence/quota.

Le menu TUI '/' doit afficher les skills après les commandes/outils, comme Claude
Code, avec sélection et invocation réelles, arguments et source/context/permissions
vérifiés. Les skills d'autres formats préservés doivent aussi être accessibles
par cette voie ; une complétion '$' Codex seule ne satisfait pas la demande.
