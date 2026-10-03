# Navigation native des agents dans le TUI embarque

Etat : implementation et verification en cours. Le binaire installe correspond encore au cycle des parametres CLI ; cet etage TUI n'est pas encore installe.

## Perimetre

`claudex agents` et `/agent-center` doivent ouvrir le tableau natif sur le vrai serveur `Embedded`. Les operations de liste, creation, lecture, reprise, renommage, interruption, archivage et suppression reutilisent les RPC existantes. Aucun faux `LocalDaemon`, lancement implicite du daemon Codex ou copie de configuration utilisateur.

Le tableau concerne les runtimes possedes par ce TUI et les historiques accessibles. Une session active appartenant a un autre processus est consultee comme un snapshot en lecture seule ; la navigation ne doit pas voler son writer. Le serveur embarque s'arrete a la fermeture du TUI. Le controle live entre terminaux et les taches detachables demandent encore un serveur partage propre a Claudex.

## Invariants de navigation

- Conserver les sessions initiales et `/new` qui n'ont pas encore de premier tour, leurs subscriptions et leurs brouillons. Une session vide ne peut pas etre reconstruite depuis un historique qui n'existe pas.
- Archiver ou supprimer une session retourne au tableau, en conservant les autres sessions gerees par le TUI.
- Fermer les conversations laterales avant la RPC destructive ; un echec d'unsubscribe bloque cette RPC.
- Esc et la fleche gauche depuis une vue writer externe reviennent au tableau. Annuler l'entree dans un dossier revient egalement au tableau.
- Garder le refus de `--no-daemon` avec un serveur explicitement distant, les verrous writer, les controles de confiance et les blocages de navigation pour permissions en attente.
- Garder le comportement de fermeture Ctrl+C du mode embarque ; l'activation de la fleche gauche ne transforme pas ce mode en daemon.

L'ancien bouton TUI qui proposait de lancer le daemon Codex disparait avec ses evenements devenus sans producteur. La CLI explicite `app-server daemon` est conservee.

## Verification et limites

Les nouvelles fixtures utilisent deux serveurs natifs dans le meme processus pour le verrou externe ; elles ne prouvent pas encore le verrou entre deux processus ou terminaux distincts. Elles utilisent et un vrai transport embarque pour la decouverte, deux sessions sans tour avec brouillons, et archive/delete avec une autre session effectivement geree par l'App. Elles ne remplacent pas `Embedded` par un marqueur `LocalDaemon`. Les tests existants de nettoyage lateral sont etendus a ce mode ; leur proxy de capture utilise toutefois un transport WebSocket vers un serveur embarque.

Les premiers huit controles fonctionnels passent, mais Nextest signale huit processus de test avec stdout/stderr encore ouverts apres leur sortie. D'apres sa [documentation officielle](https://nexte.st/docs/features/leaky-tests/), cela detecte notamment des sous-processus qui conservent ces handles. Ces signalements persistent sous un profil HOME/USERPROFILE isole : 123 controles executes, 121 reussites dont 44 marquees leaky, et deux goldens intentionnelles a actualiser (tableau natif et raccourci gauche). L'origine n'est pas demontree ; le delai de detection n'est pas assoupli.

La saisie avant la reponse de `thread/start`, les attachments et pastes, deux profils de permissions distincts et une approbation active en arriere-plan ne sont pas prouves par ces quatre nouvelles fixtures. Les contraintes Claude des profils restent refusees tant que l'heritage durable et les selecteurs d'outils ne sont pas acheves. Cet etage est un gestionnaire de sessions natif par processus, pas la parite complete de Claude Code.

La revue adversariale sequentielle de cet etage ne releve aucun P0/P1. Elle confirme le retrait des evenements daemon sans appelant et la conservation des gardes de permissions et de nettoyage lateral. Elle reste une revue de code, pas une preuve interactive.

## Journaux de ce cycle

- `tests-embedded-agents-isolated-targeted-cycle3.log` : 123 executions, 121 reussites (44 leaky), deux goldens intentionnelles.
- `tests-embedded-agents-snapshots-cycle5.log` : six controles repris, six reussites (quatre leaky). Le cycle4 a compile avant la correction inline complete et ne valide pas cette correction.
- `tests-embedded-agents-tui-package-cycle1.log` : 5555 executions, 5541 reussites (six lentes, une flaky, 273 leaky), 12 echecs, deux expirations, huit ignores ; suite non verte.
- `tests-embedded-agents-scope-cycle6.log` : dix controles repris apres correction des attentes Esc, banner et deux pieds de page, dix reussites (trois leaky). Pas de nouvelle suite complete verte.

Les autres echecs concernent notamment les frames curseur Windows, une attente de header startup, le format numerique de credits, un indicateur de progression, le catalogue d'outils du recap et deux fixtures worktree. Ils ne sont pas corriges par une acceptation globale des snapshots.

La premiere tentative de suite CLI de cet etage (`tests-embedded-agents-cli-package-cycle1.log`) s'arrete pendant la compilation : espace disque insuffisant, aucun test execute. `cargo clean --profile dev-small -p codex-core -p codex-tui` libere 8,4 Gio de caches du fork (`clean-embedded-agents-before-cli-cycle1.log`). La reprise utilise CARGO_INCREMENTAL=0, uniquement pour cette verification locale, sans changer le code ni la configuration utilisateur.

La reprise CLI sans cache incremental (`tests-embedded-agents-cli-package-cycle2.log`) termine : 473 executions, 464 reussites (une lente), sept echecs, deux expirations, deux ignores. Les cinq fixtures app-server passent. L'ancienne attente de refus du tableau local est corrigee puis recontrolee dans le cycle cible suivant ; les autres echecs daemon/onboarding, worktree, exec-server, queue et permissions cloud restent ouverts. Les cas --no-daemon locaux sont limites a Unix/Windows pour garder les fixtures portables. Les quatre .snap.new non acceptees et l'ancien pending-snap inline sont conserves sous `.build-tools/embedded-agents-baseline-snapshots`.

`tests-embedded-agents-cli-scope-cycle3.log` : 320 controles executes, 320 reussites, 155 ignores par le filtre. Ce cycle couvre les 314 tests non ignores du binaire, les cinq fixtures app-server et le garde queue/remote corrige. Aucun nouveau passage de la suite CLI complete n'est revendique.

Formatage et Clippy cibles TUI+CLI reussis (`fmt-embedded-agents-cycle1.log`, `fix-embedded-agents-cycle1.log`, `fmt-embedded-agents-cycle2.log`). Clippy fusionne seulement le if du garde --no-daemon/--remote ; aucun nouveau test apres ces modifications mecaniques. Le formatage preexistant de event_dispatch est isole dans un commit de style, les nouvelles fixtures dans un commit de tests, puis le comportement dans une tranche distincte. La revue de l'index confirme que ce commit de style conserve tous les identifiants, litteraux et anciennes branches fonctionnelles.
