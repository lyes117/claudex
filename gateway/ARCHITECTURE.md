# Architecture effectivement installée

```text
claudex.cmd
  -> launcher.mjs
  -> Claude Code original (console héritée)
       -> /v1/messages, SSE : passerelle locale authentifiée
            -> ChatGPT /backend-api/codex/responses
            -> Z.ai /api/anthropic/v1/messages
       -> MCP claudex-images
            -> transport images natif Codex / ChatGPT
```

Claude Code possède l'unique boucle agentique : exécution des outils locaux,
permissions, workflows JS, contexte et sous-agents. La passerelle traduit les
messages, appels d'outils et événements SSE ; elle ne lance pas une seconde
boucle Codex. Elle utilise uniquement le converter épinglé
`@caixiaoshun/claudex@1.0.12`, sous GPL-3.0, et les modules natifs Node.

Les noms affichés et identifiants de sélection portent les modèles réels et
leurs efforts. Les rôles utilisent les identifiants personnalisés
`claudex/deep`, `claudex/balanced` et `claudex/fast` ; les alias historiques
restent reconnus pour les workflows existants et leur historique.
Pour les modèles OpenAI dont le catalogue permet au moins 800k, les lignes
modelPicker restent personnalisées et `CLAUDE_CODE_MAX_CONTEXT_TOKENS=800000`
déclare la fenêtre native. `autoCompactWindow=720000` conserve la compaction.
GLM et les modèles sans ce maximum gardent `behavesAs` Haiku, donc 200k.
Le maximum local GPT est 872k ; une requête ChatGPT réelle de 805 028 tokens
a validé le budget conservateur de 800k. Aucune limite fictive de 1M n'est ajoutée.
Les identifiants automatiques des rôles sont désormais distincts des modèles
fixes ; les entrées `auto` autorisent le repli, les identifiants fournisseur
explicites le refusent. Le launcher démarre en `bypassPermissions` natif sauf
mode explicitement demandé ou `--restricted`. Règles, hooks et politiques
gérées restent ceux du harness. Voir [ROUTING.md](ROUTING.md) pour les règles,
mesures, limites des gates et algorithmes ML étudiés.

L'état chiffré de raisonnement OpenAI est conservé comme signature opaque
dans les blocs thinking, puis restitué au fournisseur au tour suivant.
Il est retiré lors d'un passage à Z.ai. Les schémas d'outils conservent les
arguments optionnels ; les résultats contenant des images restent multimodaux.
Un flux tronqué ou un appel d'outil malformé produit une erreur, jamais une
fausse fin réussie. Le cache est compté séparément des tokens d'entrée nouveaux.

L'authentification OpenAI vient de l'installation officielle Codex ; le
renouvellement passe par account/read du Codex app-server officiel.
Z.ai réutilise exclusivement l'identifiant Coding Plan déjà présent dans
Claude/ZCode ou CLAUDEX_ZAI_TOKEN. Aucun identifiant fournisseur n'est recopié
dans la configuration de la passerelle. Le listener est limité à 127.0.0.1,
avec un secret local aléatoire ; pas de journal de prompts, headers ou réponses.

Le daemon persiste après la fermeture d'un terminal pour permettre plusieurs
sessions. Le port sauvegardé et le listener de l'OS assurent son unicité ; aucun
verrou fichier ne bloque un redémarrage après crash. Les contrôles live utilisent
CLAUDEX_GATEWAY_STATE_DIR pour isoler leur daemon de celui de l'utilisateur.

Les images passent par un MCP stdio minimal : génération, édition de références
inspectées et transparence, puis sauvegarde exclusive dans le dossier choisi.
L'annulation est propagée ; aucune relance automatique de génération coûteuse.

## Sources consultées

- [Claude Code : protocole gateway](https://code.claude.com/docs/en/llm-gateway-protocol)
- [Modèles, noms affichés et contexte](https://code.claude.com/docs/en/model-config)
- [Workflow JS natif](https://code.claude.com/docs/en/workflows)
- [MCP Claude Code](https://code.claude.com/docs/en/mcp)
- [Z.ai Coding Plan avec Claude Code](https://docs.z.ai/devpack/tool/claude)
- [OpenAI : recherche hébergée et filtres](https://developers.openai.com/api/docs/guides/tools-web-search)
- [Converter réutilisé et licence](https://github.com/caixiaoshun/claudex)

Les transports d'abonnement et images ont été vérifiés dans la source locale
du fork Codex (`codex-api/src/endpoint`, `codex-api/src/images.rs`,
`core/src/tools/hosted_spec.rs`, `tools/src/tool_spec.rs`). Les capacités de
l'API publique OpenAI ne sont pas supposées identiques à celles du backend
ChatGPT : max_tool_calls y est notamment refusé et contrôlé localement.
