# Référence Claude Code inspectée

Le dépôt public officiel est cloné dans `C:\Users\lyesb\claudex-reference\claude-code`, commit `1c229fcd1e1e4e452e29a8f116b45fe4cfe2c528`. Son état Git est propre. L'installation Claude déjà présente répond `2.1.287`. Aucun plugin de ce clone n'a été exécuté ou activé globalement.

L'inventaire compte 1525 fichiers suivis, dont 1290 sous `mods/` et 149 sous `plugins/`. Le dépôt publie des extensions et des contrats ; il ne fournit pas un moteur CLI complet reconstructible avec un manifeste racine. La licence racine reste soumise aux conditions commerciales Anthropic. Le clone sert de référence pour une implémentation originale des formats et comportements.

## Contrats utiles et preuves

| Référence locale | Comportement à adapter | État Claudex |
| --- | --- | --- |
| `mods/types/claude-code.d.ts`, type `Workflow` vers la ligne 12574 | Script, nom, arguments JSON, fichier prioritaire, reprise d'un run de la même session | CLI JS réel et reprise validés ; outil conversationnel natif absent |
| Même fichier, résultat `Workflow` vers la ligne 13132 | Identité de tâche, nom, run-id, fichier de script persistant | Journal local présent ; association native complète au parent à terminer |
| Même fichier, `prompt.compose` vers la ligne 6665 et `prompt.section` vers 7133 | Composition par sections avec frontières de cache et faits de session | Contrats lus ; extension de composition compatible non implémentée |
| Même fichier, `Agent`, `SendMessage`, `TaskStop` | Délégation, communication, surveillance et arrêt | Mécanismes natifs Codex conservés ; équivalence complète non démontrée |
| `plugins/feature-dev/commands/feature-dev.md` | Phases de découverte, exploration, architecture, implémentation et revue | Format de commande lu en place ; nouveau catalogue TUI en validation |
| `plugins/plugin-dev/skills/agent-development/references/agent-creation-system-prompt.md` | Prompt publié du générateur d'agents | Référence disponible ; pas le prompt système principal complet |

Des prompts de plugins sont publiés. Le texte complet des prompts système principaux n'est pas établi par ce clone : publier une interface de composition ne démontre pas leur contenu effectif. Les interfaces de fonctions de hooks sont aussi indiquées comme accès anticipé dans `mods/README.md` ; aucun accès fonctionnel à ce runtime n'est présumé dans Claudex.

## Prochaines preuves nécessaires

1. Catalogue et invocation de commandes Claude réels, avec permissions et source relues au serveur, annulation et brouillon conservé.
2. Plafonds d'outils des profils Claude effectivement appliqués, y compris après reprise froide, avant d'accepter les agents actuellement refusés.
3. Outil `Workflow` natif : hôte JS borné, sous-agents de la session, parallélisme, schémas, événements et reprise liée au parent.
4. Couche originale de prompts d'orchestration : choix des agents, contrats de sortie, phases, revue adverse et reprise ; mesurer son comportement avec de vrais scénarios, sans annoncer la copie du moteur ou des prompts internes.
5. Hooks avec argv structuré, payloads exacts, délais et identité de confiance ; un test du parseur ne prouve pas l'exécution d'un plugin existant.

Sources publiques : [dépôt officiel](https://github.com/anthropics/claude-code), [options de prompts](https://code.claude.com/docs/en/cli-reference), [interfaces publiées](https://github.com/anthropics/claude-code/blob/1c229fcd1e1e4e452e29a8f116b45fe4cfe2c528/mods/types/claude-code.d.ts).

## Vérification du contrat Workflow, 3 octobre

La [documentation Workflow officielle](https://code.claude.com/docs/en/workflows) confirme l'exécution en arrière-plan, les phases, la reprise dans la même session et le cache ordonné. Le panneau permet de suivre les agents et de contrôler les runs. Ces comportements servent de référence pour le raccord aux agents natifs Codex ; le clone ne fournit pas leur moteur complet.

Le fichier `film.workflow.js` existant mesure 305 162 octets pour 3 491 lignes et comporte notamment un groupe de cinq juges. Le brouillon privé d'hôte était limité à 16 Kio et quatre appels par groupe. Ses schémas ouverts et propriétés facultatives sont également incompatibles avec la validation stricte actuelle du bridge. La taille du script local doit être dissociée du budget du contexte modèle ; taille du groupe et concurrence doivent aussi être dissociées. Les adaptations seront réalisées côté Claudex, avec validation du schéma original, sans modifier ce workflow.

Un hôte V8 privé et un superviseur Windows sont en développement dans des dossiers de staging distincts. Ils ne sont pas activés par le rebuild du catalogue et de l'accueil. La compatibilité native du film, le déclenchement conversationnel et la reprise froide restent non démontrés.
