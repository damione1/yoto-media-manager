---
title: Garder l’application à jour
order: 10
---

# Garder l’application à jour

YotoMediaManager cherche les nouvelles versions à son ouverture. Quand une nouvelle version est disponible, **Mises à jour** dans la barre latérale permet de la télécharger sans quitter l’application.

Vous pouvez aussi ouvrir **Mises à jour** et choisir **Rechercher une mise à jour** à tout moment.

1. Choisissez **Télécharger la mise à jour**. Vous voyez la progression du téléchargement et pouvez l’annuler.
2. Attendez que l’application prépare le téléchargement. Votre version actuelle continue de fonctionner jusqu’à ce que vous choisissiez d’installer.
3. Terminez les imports, les préparations audio ou les envois vers Yoto en cours, puis choisissez **Installer et redémarrer**.

L’application redémarre avec la nouvelle version. Votre médiathèque audio, vos playlists et vos connexions de compte sont conservées.

Si une autre copie de l’application est ouverte, fermez-la avant d’installer. Si l’application ne peut pas installer la mise à jour dans son dossier actuel, déplacez-la dans un dossier où elle peut écrire ou téléchargez la nouvelle version depuis les [versions publiées sur GitHub](https://github.com/damione1/yoto-media-manager/releases).

Les versions précédentes s’installaient sous le nom **Yoto**. Sur macOS, la mise à jour installe **YotoMediaManager** dans le même dossier et retire l’ancienne application **Yoto**. Sur Windows, le prochain installateur déplace l’application dans son nouveau dossier et renomme son raccourci du menu Démarrer. Dans les deux cas, votre médiathèque et votre connexion sont conservées.

Une version que vous avez compilée vous-même affiche la version **nightly**. Elle ne cherche pas de mise à jour d’elle-même et n’est jamais remplacée par une mise à jour ; téléchargez une version publiée sur GitHub Releases pour en changer.

Si un téléchargement ne peut pas être vérifié, l’application garde votre version actuelle et affiche une erreur. Choisissez **Réessayer** pour vérifier de nouveau.

## L’outil YouTube

L’outil utilisé pour les imports YouTube est mis à jour séparément, sans nouvelle version de l’application : l’application cherche une version plus récente une fois par jour et chaque fois qu’un import échoue parce que YouTube a changé. Un téléchargement n’est utilisé qu’après vérification de sa somme de contrôle publiée et s’il démarre correctement ; sinon l’application garde la version qu’elle a déjà. `Compte` → `Mode avancé` affiche la version utilisée et permet de la mettre à jour à la demande avec `Mettre à jour l’outil YouTube`.
