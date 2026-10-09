# Routing : recherche et comportement livré

## Conclusion pour Claudex

Le routing ML existe. Aucun résultat publié ne démontre cependant une qualité
garantie pour notre couple GPT/GLM, nos abonnements et les agents de StudyShare.
La version livrée améliore la disponibilité et l'effort avec des règles lisibles.
Elle n'est pas un routeur ML entraîné et ne prétend pas mesurer la qualité
sémantique d'un résultat à partir d'un simple HTTP 200.

## Approches étudiées

| Approche | Décision | Intérêt pour Claudex | Travail nécessaire |
|---|---|---|---|
| RouteLLM | Prédit la préférence entre un modèle fort et un modèle faible ; factorisation matricielle, classement pondéré ou classificateur | Candidat pour choisir GPT ou GLM avant une tâche | Calibrer sur nos tâches ; fournir des embeddings compatibles avec nos contraintes d'abonnement |
| MESS+ | Apprend en ligne des probabilités de satisfaction et optimise sous contraintes de service | Candidat quand on dispose d'un retour de qualité fiable après chaque tâche | Adapter le coût aux quotas, les fournisseurs à nos transports, et fournir les résultats des gates |
| Cascade routing | Combine choix initial et escalade après estimation de qualité | Adapté aux frontières entre phases d'un workflow | Ne pas rejouer des effets déjà exécutés ; disposer d'un estimateur de qualité valide |
| LLMRouterBench | Compare plusieurs méthodes avec un protocole commun | Utile pour choisir le benchmark, pas comme gateway prête à installer | Reproduire une évaluation sur nos tâches et modèles |

Sources primaires :

- [RouteLLM, code des auteurs](https://github.com/lm-sys/RouteLLM)
  et [article](https://arxiv.org/abs/2406.18665). Le dépôt recommande de calibrer
  le seuil sur des requêtes représentatives. Son chemin par défaut pour les
  embeddings des routeurs MF/SW demande une clé API OpenAI : il n'est donc pas
  installé tel quel dans Claudex, qui utilise uniquement les abonnements.
- [MESS+, NeurIPS 2025](https://papers.neurips.cc/paper_files/paper/2025/hash/4dd1d9b841712bd37b833559f041530c-Abstract-Conference.html)
  et [code](https://github.com/laminair/mess-plus). Les garanties de service
  dépendent des hypothèses du système et du signal de satisfaction ; elles ne
  garantissent pas qu'un agent produira toujours un film ou un article correct.
- [Cascade routing, article](https://arxiv.org/abs/2410.10347)
  et [code ETH](https://github.com/eth-sri/cascade-routing). Les auteurs identifient
  la qualité des estimateurs comme un facteur déterminant.
- [LLMRouterBench, article 2026](https://arxiv.org/abs/2601.07206)
  et [code](https://github.com/ynulihao/LLMRouterBench). Sous son évaluation unifiée,
  plusieurs méthodes récentes ne surpassent pas de façon fiable une baseline
  simple. Un algorithme plus complexe n'est donc pas une amélioration démontrée.

## Routing effectivement actif après rechargement

- Les options `/model` avec `auto` utilisent des alias de rôles et autorisent
  le repli. Sans `auto`, le fournisseur/modèle reste fixe. Ces identifiants
  sont distincts même lorsque le modèle principal est le même.
- `opus` garde le GPT principal, effort élevé ; `sonnet` le même GPT, effort
  moyen ; `haiku` conserve le GLM Flash configuré.
- L'effort explicitement demandé par le harness prime. Sans effort demandé,
  les requêtes marquées `auxiliary` par Claude Code utilisent un effort faible
  sur le même GPT : réduction du raisonnement inutile, sans changer de modèle.
- Les images et recherches hébergées imposent OpenAI. Un GLM choisi par un
  identifiant explicite incompatible produit une erreur claire.
- Les refus 429/503 permettent un repli avant le début du flux : OpenAI vers
  GLM-5.3 configuré ; GLM vers le GPT principal. Les modèles explicitement
  sélectionnés par identifiant fournisseur/modèle, la compaction,
  les images et recherches hébergées ne changent pas de fournisseur en repli.
- Un contexte OpenAI volumineux ne passe pas à GLM en cas de quota épuisé.
  Le garde conservateur exige octets UTF-8 de la requête + allowance de sortie
  au plus 200k ; il peut refuser un repli pourtant assez petit en tokens.
  Les erreurs de dépassement reconnues deviennent une phrase constante
  `Prompt is too long`, sans recopier le contenu privé du fournisseur, pour
  permettre la récupération native par compaction.
- Après un refus, le couple fournisseur/modèle est temporairement écarté selon
  `Retry-After` (secondes ou date HTTP), entre 5 et 900 secondes, ou 30 secondes
  sans délai valide.
  Si toutes les routes éligibles sont écartées, une erreur 503 avec délai est
  renvoyée sans consommer un nouvel appel fournisseur.
- Aucune réponse déjà commencée n'est remplacée par une réponse d'un autre
  fournisseur. Les sorties d'outils malformées et flux tronqués échouent
  explicitement. Les vérifications métier des workflows restent nécessaires.
- Les receipts internes enregistrent modèle, effort, rôle, catégorie native,
  motif de routing, durée, premier événement et compteurs réels disponibles.
  `usage: null` signifie que le fournisseur n'a pas donné de compteurs.
  Le premier événement n'est pas nécessairement le premier token visible.
  Aucun prompt, réponse, header ou identifiant de compte n'est enregistré.

## Gates avant d'activer un routeur appris

1. Vérifier les capacités : outils, images, recherche, contexte, sorties
   structurées et contraintes du workflow. Ces gates priment sur le score ML.
2. Mesurer un succès indépendant : dataset/provenance et Refutation/Gate pour
   une étude ; fichiers, rendu et contrôles du moteur pour un film. Une réponse
   JSON valide ou un modèle qui affirme « réussi » ne suffit pas.
3. Évaluer sur des tâches séparées des données de calibration : proportion
   de tâches acceptées, appels et tokens par tâche acceptée, latence et taux de
   reprise. Les tokens OpenAI et Z.ai ne sont pas supposés valoir le même quota.
4. Comparer au routing par rôles actuel. Garder le modèle fort par défaut
   quand le candidat n'a pas assez de preuves ou échoue à la gate de qualité.
5. Escalader à une frontière sûre du workflow, sans répéter une publication,
   génération d'asset ou écriture déjà effectuée.

Recommandation : commencer par RouteLLM calibré hors ligne si les traces de
tests montrent une opportunité réelle ; considérer une politique apprise en
ligne ensuite, quand les gates peuvent fournir un retour fiable. Aucun modèle
de routing, embedding, entraînement GPU ou appel de juge supplémentaire n'a été
installé ou lancé dans cette livraison.

Le routeur BERT local est une autre option RouteLLM à évaluer pour éviter l'API
d'embeddings ; son installation et sa calibration restent nécessaires.

## Permissions du harness

`claudex` démarre maintenant en mode natif `bypassPermissions`, demandé
par l'utilisateur pour ses workflows autonomes. `--permission-mode manual`,
`auto`, `dontAsk` ou `--restricted` explicitement fournis restent prioritaires.
Le TUI original, ses hooks et la commande `/permissions` sont conservés.
Le changement supprime les confirmations ordinaires ; il ne supprime pas les
règles explicites `ask`/`deny`, les politiques gérées ou les interactions qui
demandent réellement une réponse. Aucun fichier de permissions global ou de
projet n'a été effacé. Voir les [modes natifs](https://code.claude.com/docs/en/permission-modes).

Les sessions déjà ouvertes gardent leur mode. Le routing d'un daemon déjà
ouvert conserve également son ancienne version jusqu'à son redémarrage.
