# E1. Erreurs rendues au modèle par les outils

Question : pour chaque erreur qu'un outil de Claw peut rendre à un modèle, le message dit-il ce
qui s'est passé, pourquoi, et quoi faire ensuite ? Claw pourrait-il réparer lui-même ?

Chaque constat est ajouté ici dès qu'il est confirmé dans le code, sous cette forme :

```
## E1-nn Titre court
- Gravité : haute | moyenne | basse
- Où : chemin:ligne
- Aujourd'hui : le message exact, et quand il arrive
- Défaut : ce qu'un petit modèle ne peut pas en tirer
- Remède : le nouveau texte, ou la reprise automatique à écrire
- État : à vérifier
```

« État » passe à « confirmé », « écarté » ou « corrigé (commit) » quand la session principale
le traite.

