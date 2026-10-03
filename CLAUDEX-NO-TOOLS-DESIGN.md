# Plafond natif sans outils

Etat : implementation en verification, non installee. Le dernier binaire installe reste celui du tableau natif (2170ca4).

## Probleme et correction

Le test natif de recap attendait tools=[], mais Read/Grep/Glob etaient annonces : le helper desactivait individuellement les anciens outils, pas les nouveaux contributeurs. Son environnement vide bloquait toutefois NativeFileTool avant toute operation filesystem ; aucune lecture de fichier personnel n'a ete demontree.

Le nouveau tools.enabled (defaut true) est resolu par le host en intersection avec ToolPolicy.allowed_tools=Some([]), sans permissions supplementaires. ThreadManager compose ce plafond avant le retour d'un runtime resident ; Session et delegate utilisent le meme resolveur. Session republie le resultat dans les attachments de startup/MCP. La politique capturee reste immutable face aux remplacements d'attachments et aux demandes plus larges des enfants.

Un false explicite de SessionFlags reste un plafond meme sous une couche legacy managed prioritaire. Les autres couches gardent leur priorite normale : un profil actif peut remplacer un defaut utilisateur. Seules les couches actives participent. Aucun assouplissement des permissions managed. Le managed_config.toml par defaut n'est pas charge sur Windows ; le test utilise un override explicite.

Le helper de titre/recap demande ce plafond en gardant les desactivations separees de hooks, MCP, plugins et environnements. Le plafond d'outils ne remplace pas ces controles de startup.

## Preuves de cet etage

- tests-no-tools-targeted-cycle1.log : compilation refusee (imports de macros, type de vecteur et trait de chemin dans les fixtures), aucun test execute.
- cycle2 : sept executions, trois reussites, quatre echecs de fixtures (profil sans chemin explicite, isolation owned et libelle custom).
- cycle3 : sept executions, six reussites, un echec de fixture broker utilisant une isolation incompatible avec start_thread_until.
- tests-no-tools-targeted-cycle4.log : sept executions, sept reussites. Trois contributeurs espion sont executes en controle positif avec un environnement valide ; les appels Read/Grep/Glob et exec hostiles sont refuses avec plafond vide et compte d'execution zero. Les enfants demandent true et remplacent leurs attachments. Deux directions de reprise warm sont couvertes.
- Le broker n'annonce que exec/wait : les trois membres filesystem sont absents et leurs tentatives JavaScript echouent sans executer les spies. Ce n'est pas un RPC hostile forge vers le broker.
- tests-no-tools-core-config-package-cycle1.log : compilation arretee sur LNK1201 faute d'espace, aucun test execute. Le nettoyage PowerShell des incrementaux est refuse par le controle automatique ; le nettoyage natif Cargo des seuls paquets CLI/app-server inutilises par ces tests libere 30,3 Gio (`clean-unused-packages-no-tools-cycle1.log`). Sources et installation preservees. Reprise de la suite dans cycle2.
- schema-no-tools-cycle1.log : just write-config-schema reussi, schema actualise. Son cache dev distinct est libere par Cargo (2,8 Gio) apres verification du chemin dans le fork ; les sources et le binaire installe sont conserves.
- tests-no-tools-tui-targeted-cycle1.log : compilation refusee, aucun test execute, car le paquet TUI ne depend pas de test_case. Deux tests Tokio explicites reprennent les memes controles direct/CodeMode avec un helper partage ; aucune dependance ajoutee. Reprise dans cycle2.
- tests-no-tools-tui-targeted-cycle2.log : 47 executions, 40 reussites (sept leaky), sept echecs de fixture car gpt-5.2 est absent du catalogue embarque. Captures ANSI, header et credits passent. Le catalogue local est corrige avec le constructeur fallback natif pour ce modele synthetique ; les attentes tools=[] et les deux modes restent inchanges.
- tests-no-tools-tui-targeted-cycle3.log : 47 executions, 47 reussites (13 leaky), 5504 filtres, 24,153 s. Titres et recaps Embedded gardent tools=[] avec des descripteurs de fixture direct et CodeModeOnly. Les assertions de contenu, schema et budgets sont conservees ; ANSI, header et credits passent. Ces tests ne sont pas des inferences ChatGPT et les handles leaky restent un diagnostic ouvert.
- tests-no-tools-core-config-package-cycle2.log : 4609 tests executes, 4587 reussites, 21 echecs, un timeout et 77 ignores (1643,324 s). Tous les nouveaux tests plafond passent aussi dans cette suite. Cette compilation precede la tranche schema native, validee separement.
- Les tests cli_stream et plusieurs Windows sandbox ont besoin du binaire de developpement `target/dev-small/codex.exe`, retire par le nettoyage Cargo cible. Ce sont des echecs de precondition de cette execution ; reconstruire ce binaire et relancer les familles concernees avant d'en tirer une conclusion sur le code. L'installation utilisateur reste intacte. Symlinks, trois snapshots de catalogue/instructions, un message de refus localise et le timeout MCP sont des diagnostics distincts, pas une validation globale verte.
- tests-no-tools-tui-package-cycle1.log : 5557 executions, 5556 reussites, un echec de snapshot de l'indicateur Working, 301 leaky et huit ignores (625,850 s). Les deux tests worktree precedemment en timeout passent dans ce cycle. Le test follow changeait animations apres construction du widget ; sa correction et la revue des snapshots sont validees separement. Aucun timeout relache.
- clean-core-tui-caches-cycle1.log : nettoyage Cargo cible core/TUI, 216 fichiers et 5,9 Gio liberes. Les sources, journaux et binaires installes sont conserves.
- tests-follow-animation-targeted-cycle1.log : un test passe avec snapshots regeneres, mais leur relecture revele l'en-tete du nouveau widget et un decalage du transcript. Ces captures intermediaires ne sont pas acceptees.
- tests-follow-animation-targeted-cycle2.log : un test passe (0,924 s) apres remplacement du welcome par une cellule vide via le helper existant. Les quatre captures finales gardent transcript, draft, caret et geometrie ; seuls le spinner et les troncatures associees changent. La reprise ciblee ne constitue pas une nouvelle suite complete verte.
- fmt-no-tools-and-schema-cycle2.log et fix-no-tools-and-schema-cycle1.log : formatage puis Clippy core/config/TUI reussis. Les changements mecaniques Bazel et l'import scenarios hors perimetre sont retires. Les revues adversariales successives ne trouvent aucun P0/P1 dans ces tranches ; les limites de preuve ci-dessous restent ouvertes.

## Limites ouvertes

Les spies sont des contributeurs synthetiques, pas NativeFileTool. Les fixtures recap/titre sur Embedded avec modeles direct et CodeModeOnly passent ; l'echec follow de la suite TUI est corrige et repris seul. La suite core/config complete n'est pas verte. Build et installation du nouveau plafond non executes.

Les catalogues TUI portent les descripteurs de mode voulus, mais tools=[] ne
constitue pas une verification independante du mode effectivement resolu : cette
attente resterait vraie si ce champ etait ignore. Le test natif du broker prouve
separement l'annonce exec/wait, l'execution JavaScript reelle et les spies a zero.
Le smoke CLI futur ne prouvera ni un broker execute ni chaque absence d'operation
filesystem ; il verifie l'inference textuelle avec le plafond demande.

La capture en memoire ne garantit pas une restauration apres fermeture : le plafond durable reste un etage distinct. Les templates de certains modeles contiennent toujours functions.exec malgre tools=[] ; aucun outil n'est reintroduit, mais ce texte peut provoquer une tentative refusee. En MAv2 avec disable_direct_message=true, le routeur exige post ; le plafond vide peut donc bloquer le tour. Le helper temporaire desactive le multi-agent et n'utilise pas cette combinaison. Ces deux limites sont issues de revue de code, pas de preuves d'inference reelle.

Deux commits coherents sont prevus : plafond generique/schema/config/warm, puis helper TUI et preuves adverses natives. Le catalogue et le dispatch restent des mecanismes Codex reels ; aucune contrainte Claude restrictive n'est activee par cette tranche.
