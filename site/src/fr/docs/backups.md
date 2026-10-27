---
title: "Sauvegarder et restaurer sa médiathèque"
navTitle: Exporter et importer votre médiathèque
description: "Exportez une sauvegarde chiffrée de votre médiathèque YotoMediaManager, conservez sa clé et importez vos fichiers sur un autre ordinateur."
order: 8
---

# Sauvegarder et restaurer sa médiathèque

Gardez une copie de votre médiathèque locale ou transférez-la vers un autre ordinateur. Cet export est distinct des playlists enregistrées dans votre [compte Yoto](account-and-sync.md).

Votre médiathèque contient votre audio original et les versions préparées. Elle est stockée localement, sous forme chiffrée. Le catalogue garde aussi des données en cache et suit les opérations Yoto ; les playlists elles-mêmes appartiennent à votre compte Yoto. Exporter la médiathèque vous donne une copie protégée, que vous pouvez importer sur une nouvelle installation ou un autre ordinateur.

## Exporter la médiathèque

1. Ouvrez `Médiathèque`, puis le menu `⋯` (`Plus d’options`) en haut à droite.
2. Choisissez `Exporter la médiathèque…`.
3. Choisissez `Choisir un dossier…` et indiquez où placer l’export, par exemple une clé USB. L’application y crée un dossier `Médiathèque Yoto` daté.
4. Le dialogue affiche une clé de récupération créée pour cet export, en huit groupes de lettres et de chiffres. Choisissez `Copier` et gardez-la dans votre gestionnaire de mots de passe, ou notez-la sur papier. Rangez-la à part de l’export.
5. Choisissez `Exporter` et attendez le résultat. Le dialogue indique où l’export a été enregistré, ou pourquoi il a échoué.

L’export comprend le catalogue et ses fichiers audio. Les sessions de connexion Yoto n’en font pas partie ; vous devrez reconnecter Yoto après l’import.

La clé de récupération est nécessaire pour ouvrir l’export. Personne ne peut l’importer sans la clé, pas même vous : un export seul ne suffit pas à récupérer votre médiathèque. Une nouvelle clé est créée à chaque export.

## Importer une médiathèque

1. Ouvrez `Médiathèque`, puis le menu `⋯`.
2. Choisissez `Importer une médiathèque…`.
3. Collez ou saisissez la clé de récupération notée lors de l’export. Les espaces et les tirets entre les groupes n’ont pas d’importance, pas plus que les majuscules. La clé est vérifiée avant toute autre chose.
4. Choisissez `Choisir l’export…` et sélectionnez le dossier de l’export (ou le dossier qui le contient).
5. Choisissez `Importer`. Chaque fichier est vérifié ; cela peut prendre quelques minutes pour une grande médiathèque.
6. Choisissez `Ouvrir la médiathèque importée` pour l’ouvrir dans une nouvelle fenêtre, puis reconnectez votre compte Yoto dans `Compte`.

L’import ne remplace jamais rien : la médiathèque importée est créée dans un nouveau dossier à côté de votre médiathèque actuelle, et la fenêtre actuelle reste ouverte. Pour la rouvrir plus tard, activez le `Mode avancé` dans `Compte` et choisissez `Ouvrir une autre médiathèque`.

## Exporter un original

Pour récupérer un seul fichier, sélectionnez-le dans `Médiathèque` et choisissez `Exporter l’original` dans `Plus d’options`. L’export produit un fichier audio lisible, avec son extension d’origine, à l’endroit que vous choisissez ; un fichier existant n’est jamais écrasé.

## Nettoyer les fichiers inutilisés

Dans le menu `⋯` de `Médiathèque`, `Libérer l’espace inutilisé` supprime les fichiers qui ne sont plus utilisés par un média, une version, une playlist ou une tâche en cours. Les fichiers encore utilisés sont conservés.

Pour supprimer un média utilisé par une playlist, retirez d’abord ses pistes. Exportez la médiathèque avant de réorganiser une grande médiathèque.
