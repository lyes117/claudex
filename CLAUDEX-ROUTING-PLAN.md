# Routage Codex + Z.ai — plan de réalisation et preuves

État du 3 octobre 2026 : recherche officielle et inventaire local effectués ; conception et revue adverse en cours. Aucun appel Z.ai effectué, aucun identifiant copié, aucun routage GLM activé. Le binaire Claudex actuellement installé continue d'utiliser Codex/ChatGPT.

## Résultat attendu

Le superviseur reste sur le moteur et l'authentification officiels Codex. Il peut déléguer une tâche bornée à un véritable enfant natif Claudex utilisant GLM-5.3 et le quota du Coding Plan Z.ai. La session principale conserve ses outils, ses permissions, ses fichiers Claude et ses capacités Codex. Chaque décision indique le fournisseur, le modèle, la raison, l'état du quota connu et les limites applicables. Une tâche inconnue, risquée ou incompatible reste sur Codex.

L'utilisateur confirme un Coding Plan Lite. Le raccord vise également Pro, Max et Team : une seule admission par défaut pour chaque offre, limites configurables puis adaptées uniquement aux preuves du service. Une connexion ZCode, un abonnement au chat Z.ai et une clé API générale ne sont pas interchangeables avec une clé Coding Plan. Les offres qui ne donnent pas accès au plan doivent rester explicitement indisponibles, sans facturation API automatique.

L'abonnement est un quota en points, avec fenêtres et limites de concurrence ; les tokens observés ne constituent pas une mesure exacte du solde du plan. Aucune promesse d'économie chiffrée avant mesure sur le compte et sur les tâches de l'utilisateur.

## Sources actuelles et contradiction à résoudre

Sources primaires consultées : [GLM-5.3](https://docs.z.ai/guides/llm/glm-5.3), [guide Codex](https://docs.z.ai/devpack/tool/codex), [FAQ](https://docs.z.ai/devpack/faq), [outils pris en charge](https://docs.z.ai/devpack/tool/others), [politique d'utilisation](https://docs.z.ai/devpack/usage-policy), [fiche du modèle publiée par Z.ai](https://huggingface.co/zai-org/GLM-5.3).

- La page GLM-5.3 indique que les comptes ayant souscrit au Coding Plan, même expiré, utilisent actuellement le protocole Chat Completions.
- Le guide Codex fournit pourtant une configuration Responses avec `https://api.z.ai/api/v1`. Ce guide ne prouve donc pas que Responses fonctionne pour ce compte.
- Le chemin Chat Completions dédié au plan est `https://api.z.ai/api/coding/paas/v4`. Le chemin API ordinaire est exclu du raccord destiné à l'abonnement. Aucun changement silencieux de chemin, fournisseur ou mode de facturation.
- Les pages limitent les avantages aux outils officiellement pris en charge. Codex est nommé ; l'éligibilité du fork Claudex n'est pas explicitement attestée. Un HTTP 200 ne prouve ni cette éligibilité ni l'imputation au quota. Ne pas usurper l'identité d'un outil reconnu.
- GLM-5.3 accepte du texte et exige le raisonnement activé ; les niveaux documentés sont `low`, `high`, `max`. Ne pas réutiliser sans adaptation les niveaux et paramètres OpenAI.
- Les limites de concurrence sont dynamiques. Les nombres de projets recommandés ne sont pas un nombre contractuel de requêtes simultanées. Départ avec un seul enfant Z.ai actif.

## Inventaire local vérifié

| Élément | Vérification actuelle | Conséquence |
|---|---|---|
| Claudex | Exécutable natif et entrée PATH utilisateur présents ; nouvelle lecture du PATH nécessaire dans le processus d'outils ancien | Préserver l'installation et sa sauvegarde |
| Codex officiel | Installation OpenAI présente | Préserver l'authentification ChatGPT |
| Claude Code | `~/.local/bin/claude.exe` présent | Aucun besoin de copier son moteur |
| ZCode | Application Windows 3.14.4 et `ZCode.exe` présents | Ne pas confondre connexion ZCode et clé utilisable par un fournisseur tiers |
| État ZCode | Dossiers CLI et v2, fichier d'identifiants v2 présents | Format/provenance de la clé de plan et validité non établis ; lecture de présence uniquement, aucun import |
| OpenCode | Dossier de configuration présent ; commande absente du PATH utilisateur courant | Présence de dossier ne prouve pas un client fonctionnel |
| Clé Z.ai | Variables standard ciblées absentes des portées examinées ; aucune clé de plan identifiée dans les champs reconnus | L'authentification réelle reste à raccorder sans secret dans arguments, logs ou documents |
| Fournisseurs du fork | `WireApi` actuel ne propose que Responses | Nécessité d'un transport natif Chat Completions ou d'une autre voie officiellement compatible et démontrée |
| Sous-agents | Préparation commune V1/V2 et gestionnaire natif existants | Réutiliser ces mécanismes ; remplacer seulement le modèle ne change pas le fournisseur |

## Benchmarks : éléments utiles et limites

La fiche Z.ai rapporte GLM-5.3 à 88,2 sur Terminal-Bench 2.1, 28,3 sur Terminal-Bench 3.0, 66,9 sur DeepSWE 1.1 et 73,0 sur Toolathlon Verified. Ces résultats sont publiés par le fabricant ; ils ne sont pas une mesure locale Claudex. Les protocoles, budgets et harness diffèrent. Plusieurs évaluations utilisent `max`, Claude Code et des durées longues : elles ne valident pas une route GLM `low` pour les tâches courtes.

Hypothèse à tester : GLM-5.3 `low` peut traiter correctement une partie des recherches de dépôt, documentations et transformations bornées, tandis que le superviseur Codex garde la planification globale, la sécurité, les décisions ambiguës et la validation finale. Une règle ne devient automatique qu'après réussite des cas correspondants.

Calibration locale prévue : tâches synthétiques sans secrets, réponses et critères indépendants du modèle, même contexte et mêmes outils, latence jusqu'au premier contenu/final, tokens déclarés par le service, erreurs, conformité et nombre de reprises. Le coût en points reste inconnu si le fournisseur ne l'expose pas. Aucun classement inventé ; aucun benchmark distant exécuté à ce stade.

## Politique initiale

1. Modes `off`, `auto` et demande explicite Z.ai. `off` conserve le comportement actuel.
2. Le superviseur fournit des métadonnées structurées de tâche ; le texte d'une instruction ne peut changer les permissions ou l'éligibilité. Pas de classificateur fondé seulement sur la longueur ou des mots clés.
3. Seules les catégories explicitement admissibles et à faible risque peuvent quitter Codex. Architecture, sécurité, migrations, opérations externes, contexte ambigu et tâches non classées restent sur Codex.
4. Un enfant Z.ai commence avec du contexte frais et borné. Pas de transfert de l'historique complet, des raisonnements, des identifiants ou de la mémoire globale du parent.
5. Images, fonctionnalités non prises en charge, modèle demandé incompatible, quota/abonnement non vérifiés ou circuit ouvert empêchent la route GLM. Une demande explicite ne contourne pas ces conditions.
6. Les permissions gérées, restrictions du rôle, hooks et sandbox sont une limite supérieure indépendante du modèle. Les enfants ne peuvent relancer un routage plus permissif.
7. Une seule tâche Z.ai simultanée initialement ; file bornée, admission avant lancement, libération garantie après erreur/annulation. Les compteurs sont partagés entre sessions si l'architecture autorise plusieurs terminaux.
8. Une erreur de quota/authentification ne redirige jamais vers une API payante. Une reprise sur Codex avant tout effet est explicite et journalisée ; après effet possible, demander une décision au superviseur au lieu de répéter aveuglément.

## Lots et dépendances

| Lot | Responsabilité/fichiers envisagés | Dépendances | Preuve exigée |
|---|---|---|---|
| R0 | Sources, inventaire, ce plan | Aucune | Sources primaires datées, inventaire sans valeurs sensibles |
| R1 | Noyau pur `model-provider-info/src/claudex_routing*` | R0 et revue du contrat | Matrice décisions et refus, endpoints trompeurs, défaut inchangé |
| R2 | Configuration typée, état fournisseur et diagnostics CLI | R1 | Schéma si ConfigToml change, priorité globale/projet/gérée, aucun secret sérialisé |
| R3 | Authentification dédiée Z.ai | R0 | Clé de plan correctement identifiée, stockage protégé/référence existante, jamais auth ChatGPT vers Z.ai |
| R4 | Requête/réponse Chat Completions native dans `codex-api` | R0, R3 pour live seulement | HTTP local réel, streaming fragmenté, UTF-8, raisonnement, appels d'outils, usage, erreurs, EOF et annulation |
| R5 | Fournisseur/capacités/catalogue GLM | R4 | Pas de découverte OpenAI avec token Z.ai, paramètres GLM exacts, outils compatibles seulement, compaction locale |
| R6 | Admission et budgets de tâches | R1 | Concurrence, quota inconnu/épuisé, réserve/libération, timeouts, file bornée, circuit et reset |
| R7 | Raccord enfants natifs V1/V2, rôles et workflows | R2, R5, R6 | Parent ChatGPT/enfant GLM distincts, politique effective avant publication, fork refusé, reprise cohérente |
| R8 | TUI et CLI | R2, R7 | `/routing`, diagnostics fournisseur/modèle/raison, changements explicites, visualisation tâches, aucune fuite |
| R9 | Calibration et revue adversariale | R4 à R8 | Fixtures indépendantes, mauvaises sorties détectées, injection/refus, pas de double effet |
| R10 | Build, installation et essais de bout en bout | R9 et conditions d'abonnement résolues | PowerShell/CMD/TUI, vrai enfant GLM, tool call autorisé, quotas observés, parent toujours ChatGPT |

R1, conception R4 et revue R0/R7 peuvent avancer en parallèle sur des fichiers distincts. Un agent garde un checkpoint du chantier TUI Claude ; un autre termine les preuves de supervision mémoire avant réaffectation. Le raccord central, les schémas, les builds et l'installation restent coordonnés pour éviter de tester des états partiellement modifiés.

## Contrat du transport à détailler avant activation

- Requêtes : mapping exhaustif des messages texte, rôles, résultats d'outils et schémas fonctionnels ; refus explicite des objets impossibles à représenter. Pas de suppression silencieuse d'outils/instructions.
- Réponses : contenu et raisonnement séparés, identifiants d'appels stables, arguments accumulés et validés après complétude, événements terminaux exacts, usage uniquement fourni par le service.
- Fin de flux : EOF sans fin reconnue = erreur, jamais succès simulé. Corps et frames bornés. Redirections refusées ; token lié au domaine et chemin approuvés.
- Annulation : abort réseau et enfants, libération de budget. Retries bornés uniquement pour erreurs et opérations sûres ; pas de répétition d'un outil à effet.
- Auth : fournisseur enfant indépendant du parent ; headers internes OpenAI, IDs d'organisation et tokens ChatGPT exclus des requêtes GLM.
- Reprise : fournisseur, identité de modèle, version de politique et capacités persistés ; aucune conversion d'une conversation Responses en Chat Completions non démontrée.

## Revue adverse et critères de livraison

Le reviewer doit tenter de casser le plan puis le code, et pas seulement approuver son intention. Cas prioritaires : rôle nommé « simple » demandant une migration ; faux endpoint avec userinfo/sous-domaine/port/query ; prompt ordonnant d'utiliser une clé OpenAI ; clé API ordinaire prise pour clé de plan ; quota inconnu présenté comme disponible ; 429 entraînant une tempête de retries ; flux coupé après un outil ; reprise qui change de fournisseur ; partage involontaire du contexte complet ; tâche GLM issue d'un enfant GLM récursif ; deux terminaux dépassant la limite ; affichage d'erreurs contenant des identifiants.

Les conclusions de la revue, les tests exécutés et les changements de conception sont inscrits ici au fur et à mesure. Une bibliothèque de décisions testée ne prouve pas le routage natif. Un serveur HTTP synthétique ne prouve pas l'authentification, l'éligibilité ou l'imputation au plan. La fonctionnalité n'est livrée que lorsque les derniers essais sont réels ; toute limite d'accès restante est affichée comme telle.

## État des lots

- [x] R0 : documentation officielle et inventaire local initial.
- [x] R1 : noyau écrit et relu ; tests ciblés réussis. Décision de candidature uniquement, sans dispatch.
- [ ] R2–R8 : raccords natifs à implémenter et vérifier.
- [ ] R9 : revue adverse du plan en cours ; calibration réelle non exécutée.
- [ ] R10 : aucune activation ni inférence Z.ai.

## Revue adverse appliquée au premier lot

- Les attestations d'éligibilité, de type de clé, de frontière de facturation et de fraîcheur du quota sont distinctes. Une réponse réussie ou le nom de l'offre ne les remplit pas.
- Un enfant neuf Codex hérite encore des instructions, des extensions et d'un catalogue parent. R7 doit préparer un snapshot dédié et imposer l'absence de ces héritages avant toute candidature ; `Fresh` seul ne suffit pas.
- Les métadonnées de tâche fournies par un modèle choisissent seulement parmi des capacités déjà accordées. Elles ne prouvent pas qu'un fichier est dépourvu de secrets. L'autorité initiale limite la route à du texte fourni et à des outils spécialisés bornés ; pas de shell arbitraire.
- La réservation de concurrence doit survivre aux terminaux multiples. Sa durée de validité ne remplace pas la preuve d'arrêt de l'enfant avant libération.
- La reprise doit valider le profil complet et sa version, pas seulement l'identifiant du fournisseur. Le délai global inclut attente, raisonnement, outils, réseau et nettoyage.
- Le décodeur de flux ne produit aucun appel d'outil complet avant confirmation terminale ; EOF seul échoue. L'ordre des événements ferme le raisonnement avant d'ouvrir un message. Raisonnement tardif et objets impossibles à représenter sont refusés.
- Les bornes du premier décodeur sont conservatrices : 8 Kio de contenu sémantique total et 1 Kio d'arguments par appel. Le constructeur de requêtes accepte actuellement des fragments de 8 Kio : l'intersection effective et le replay du raisonnement sont à résoudre dans R5, avant une boucle d'outils réelle.

Le premier `cargo check` ciblé après gel passe. Clippy a trouvé une simplification booléenne, corrigée sans changement de comportement. La validation suivante passe : 238 tests des bibliothèques `codex-model-provider-info` et `codex-api`, zéro échec, puis Clippy ciblé avec warnings interdits. Les DTO de requête et le décodeur de flux GLM sont testés ; le transport réseau et le dispatch natif restent à raccorder. Aucun résultat de compilation ne démontre une connexion Z.ai.

## Reprise et vérification des sources — 4 octobre 2026

Les pages officielles [GLM-5.3](https://docs.z.ai/guides/llm/glm-5.3) et
[Codex](https://docs.z.ai/devpack/tool/codex) ont été relues. La contradiction
de protocole persiste : la première réserve actuellement Chat Completions aux
comptes ayant souscrit un Coding Plan, alors que le guide Codex propose Responses.
Le raccord prévu reste donc Chat Completions sur le chemin dédié au plan,
sans présumer que Responses fonctionne pour le compte de l'utilisateur.
Texte seul, raisonnement activé et niveaux `low`, `high`, `max` sont confirmés
par la documentation du modèle. Aucun appel d'inférence distant exécuté.

La cartographie du code actuel confirme qu'il manque un endpoint de transport
Chat Completions et un framer SSE brut borné devant le décodeur existant.
`ModelClientSession::stream` n'a encore qu'une branche Responses. Une première
tranche privée de transport HTTP local peut être vérifiée sans changer le parent
ChatGPT ; elle ne prouvera ni dispatch enfant, ni authentification ou quota réel.

### Transport dédié intégré, route native encore ouverte — 4 octobre

Le client Coding Plan, le framer SSE brut borné et le stream annulable ont été
appliqués après double revue et vérification des empreintes. Les 30 fixtures
ciblées passent au cycle3 (326 autres tests exclus), sans inférence distante.
Les deux corrections concernent uniquement les fixtures MIME/backpressure.
L'interface R5-A expose le client et les limites au core sans ajouter de branche
WireApi, de credential resolver, de provider ni de dispatch enfant.

Le contrat R5-B à R5-F est préparé dans
`.build-tools/recovery-zai-native-runtime-cycle4/CONTRACT.md` : admission host,
identité indépendante, replay du reasoning, raccord V1/V2, entitlement et quota.
Les modules PC/MCP existants doivent être attestés sur le binaire final ; le
transport texte seul ne prend pas en charge images/CUA/history parent et refuse
leur admission. Aucune disponibilité de clé ou de modèle pour le compte n'est
présumée. Le guide de fact checking reste la référence de clôture.

### Modèles et crédits : source officielle relue le 4 octobre

L'[overview officiel](https://docs.z.ai/devpack/overview) annonce désormais
GLM-5.3 et GLM-5.3-Flash pour tous les plans. Les anciens identifiants 5.2/5.1
sont remappés vers 5.3 et 4.7 vers Flash. La première tranche privée, fixée sur
5.3/low, ne satisfait donc pas encore le choix entre plusieurs modèles demandé.

Les crédits se calculent sur les tokens input/cache/output pondérés, puis divisés
par 10000 : multiplicateurs 5.3 = 6.9/1.7/24 et Flash = 2.3/0.56/8. Le plan
Lite annoncé donne 2000 crédits/5h et 10000/semaine. Ces limites publiées ne
prouvent ni le solde du compte, ni son état courant, ni une quantité fixe de tokens.
Les promotions et heures creuses sont datées ; elles ne doivent pas être figées
dans une estimation permanente du routeur.

La [documentation Flash](https://docs.z.ai/guides/vlm/glm-5.3-flash) annonce des
entrées multimodales et confirme Flash disponible sur Coding Plan, FlashX exclu.
Cela ne rend pas l'adaptateur Claudex multimodal : celui-ci reste texte seul,
avec refus de CUA/images. La conservation PC via Codex et l'ajout éventuel d'un
profil Flash exigent leurs propres chemins exécutés et preuves. Ni cette page,
ni une compatibilité annoncée pour Codex ne prouvent l'éligibilité du fork.
