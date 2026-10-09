# Claudex : Claude Code original, OpenAI et Z.ai

Installation locale : `C:\Users\lyesb\claudex-gateway`.
Commande PowerShell et CMD : `claudex`.
Le fork Rust existant et sa commande `claudex` sont conservés séparément.

## Utilisation

Depuis ton projet, lancer :

```powershell
claudex
```

Le terminal, les workflows JavaScript, sous-agents, hooks, skills, MCP et
permissions sont ceux du véritable exécutable Claude Code installé.
Les nouveaux lancements utilisent `bypassPermissions` pour les workflows
autonomes. `claudex --permission-mode manual` rétablit les confirmations ;
un mode explicitement demandé reste prioritaire. Les règles `ask`/`deny` et
politiques gérées restent appliquées ; `/permissions` reste dans le TUI.
`/model` présente les vrais modèles ; `/workflows` ouvre la vue native.
Les options marquées `auto` autorisent le routing/repli ; les modèles sans
`auto` restent sur le fournisseur explicitement choisi.
Les anciens scripts `model: "opus"`, `"sonnet"`, `"haiku"` restent utilisables.

| Rôle historique | Inférence réelle | Effort |
|---|---|---|
| opus | ChatGPT : gpt-6.1-sol | high |
| sonnet | ChatGPT : gpt-6.1-sol | medium |
| haiku | Z.ai Coding Plan : GLM-5.3-Flash | fournisseur |
| repli quota OpenAI | Z.ai Coding Plan : GLM-5.3 | fournisseur |

`/model` permet aussi de sélectionner GLM-5.3 et les modèles du catalogue
Codex local. Le repli concerne uniquement les refus 429/503 avant le début
du flux ; les images et recherches hébergées restent sur OpenAI.
Le repli est également possible de GLM vers GPT lorsque le Coding Plan refuse
l'appel. Les routes ayant refusé sont temporairement écartées ; les auxiliaires
natifs utilisent un effort faible sur le même GPT sans effort explicite demandé.
Il n'y a aucune utilisation de clé API OpenAI payante.
Les limites et la recherche ML sont détaillées dans [ROUTING.md](ROUTING.md).

```powershell
claudex doctor
```

La configuration des routes est dans
`C:\Users\lyesb\.claudex\gateway\settings.json`.
Après sa modification, fermer les workflows/sessions utilisant la passerelle,
puis exécuter `claudex gateway-stop` et relancer `claudex`.
Cette commande d'arrêt interrompt les appels actifs : ne pas l'utiliser pendant
un workflow. Les fichiers runtime et claude-settings contiennent un secret local
et ne doivent pas être partagés.

## Corrections et vérifications

Les deux causes de l'échec WebSearch ont été corrigées : choix d'outil hébergé
traduit correctement, puis conservation des éléments SSE lorsque l'enveloppe
finale du service ChatGPT omet ses résultats. Les citations sont rendues dans
le format attendu par Claude Code. Les restrictions de domaines sont transmises.
La limite max_uses est annoncée au modèle et surveillée dans le flux : un
dépassement annule la réponse. Le service d'abonnement refuse max_tool_calls ;
un appel hébergé peut déjà avoir commencé avant l'annulation locale.

WebFetch reste natif : Claude Code récupère la page puis demande son résumé
au modèle de fond. Le MCP Chrome reste disponible. Pendant le test StudyShare,
l'agent a appelé Chrome après les erreurs WebSearch ; cela ne venait pas d'une
conversion de WebFetch en navigateur.

Vérifications réelles du développement : conversation OpenAI, Read/Write,
sous-agent, création et exécution d'un workflow JS natif à deux agents
OpenAI/GLM, inférence et outils GLM-5.3/Flash, génération et édition d'une image
par abonnement ChatGPT, WebFetch. Le TUI original a été ouvert et son sélecteur
de modèles inspecté. Les preuves synthétiques restent dans `artifacts/`.
Validation initiale du 7 octobre 2026 : cinq tests locaux réussis ; WebSearch natif
réussi avec deux appels hébergés observés ; workflow natif sonnet/haiku
réussi avec appels réels OpenAI/Z.ai et fichier résultat vérifié.
La passerelle utilisateur a été rechargée et sa recherche vérifiée en HTTP 200.
Le workflow complet StudyShare, jusqu'à sa publication, n'a pas été exécuté
par ce développement ; aucune publication n'a été effectuée.
Validation du routing et du mode autonome : neuf tests locaux réussis ; test
réel Write dans `.claude/` et Bash sans confirmation ; workflow JS natif avec
sous-agents sonnet/OpenAI et haiku/GLM, fichier résultat vérifié. Les sessions
utilisateur déjà actives ont été conservées sans redémarrage.

Validation du 8 octobre : onze tests locaux réussis ; abonnement GPT-6.1 Sol
acceptant 805 028 tokens d'entrée et réponse complète. Le lancement Claude Code
annonce 800k ; Write/Bash sans confirmation et workflow JS natif sonnet/haiku
réussis. Les sous-agents annoncent 800k pour OpenAI et 200k pour GLM.

```powershell
cd C:\Users\lyesb\claudex-gateway
npm test
node test/live.mjs search
node test/live.mjs workflow
node test/live.mjs progress
node test/live.mjs permissions
node test/context-live.mjs
```

`npm test` est local et sans inférence. Les contrôles live utilisent les quotas
d'abonnement et un espace de travail/passerelle séparés. Le contrôle `search`
exige un véritable appel de recherche observé, pas seulement un texte réussi.
Le contrôle `progress` exige des deltas de raisonnement ou d'arguments d'outils
avant la réponse finale, avec un véritable Read/Write dans l'espace synthétique.
Le contrôle `context-live` envoie plus de 800k tokens synthétiques une fois ;
il consomme le quota d'abonnement et exige les compteurs réels du fournisseur.

Le flux transmet les résumés de raisonnement publics et les arguments d'outils
au fil de l'eau. Les compteurs exacts OpenAI arrivent à la fin de chaque réponse ;
le zéro d'un bloc encore en cours n'indique donc pas une absence d'inférence.
Le test réel a observé 78 deltas avant completion et 131 tokens de sortie.
La passerelle persistante d'une session déjà ouverte conserve son ancienne
version. Après la fin des workflows en cours, exécuter
`claudex gateway-stop`, puis relancer `claudex` depuis le dossier
du projet. Ne pas arrêter la passerelle pendant un workflow actif.

## Limites connues

- L'accès ChatGPT réutilise le transport natif de Codex et son OAuth existant.
  C'est une compatibilité avec ce backend, susceptible de changer ; elle n'est
  pas présentée comme une API publique générale de l'abonnement ChatGPT.
- Le TUI original peut afficher « API Usage Billing » et des estimations de
  coûts Anthropic. Ces chiffres ne décrivent pas la facturation du transport.
- Le mode de permissions reste celui de Claude Code. L'auto mode peut être
  indisponible pour un modèle GLM sélectionné comme agent principal.
- Le GPT principal est budgété à 800k, avec compaction proactive à 720k.
  GLM conserve son profil 200k et le seuil de compaction est plafonné à ce profil.
  Les anciens alias restent routés mais une session reprise doit sélectionner
  `--model claudex/deep` pour charger le nouveau profil. Compter les tokens
  sans appel fournisseur reste une estimation. Les documents PDF bruts sont
  refusés explicitement ; fournir leur texte ou leurs images.
- Les outils MCP sont chargés directement : la recherche différée de tools
  Anthropic est désactivée pour éviter des références incompatibles.
- Le moteur de workflow est conservé ; obtenir exactement les mêmes décisions
  avec un autre modèle se mesure sur tes workflows réels.

Architecture et sources : [ARCHITECTURE.md](ARCHITECTURE.md).
