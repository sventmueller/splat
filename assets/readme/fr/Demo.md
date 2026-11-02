# Le café filtre parfait

Un lent rituel du matin en quelques étapes. Écrivez-le dans la **source** à gauche ou directement dans l’*aperçu* à droite : les deux restent synchronisés, et ==chaque modification est un changement propre== du Markdown.

> [!TIP]
> Une eau juste en dessous de l’ébullition, vers 94 °C, révèle la douceur sans amertume.

## Le ratio

La plupart des recettes retiennent une part de café pour seize parts d’eau :

$$
m_{\text{eau}} = 16 \times m_{\text{café}}
$$

Ainsi, $20\,\text{g}$ de café demandent $320\,\text{g}$ d’eau.

## Étape par étape

```mermaid
flowchart LR
    A[Moudre] --> B[Infuser] --> C[Verser] --> D((Savourer))
```

## Méthodes d’extraction

| Méthode     | Mouture    | Durée |
|:------------|:----------:|------:|
| Café filtre | Moyen-fine |  3:30 |
| Piston      | Grossière  |  4:00 |
| Espresso    | Fine       |  0:28 |

## Avant de commencer

- [x] Des grains fraîchement torréfiés :coffee:
- [x] Un moulin à meules
- [ ] Une bouilloire à bec col de cygne

```python
def water_for(coffee_g: float, ratio: int = 16) -> float:
    return coffee_g * ratio
```

Préinfusion
: Le premier jet d’eau qui laisse le café moulu libérer son gaz.[^1]

[^1]: Les grains frais gonflent le plus ; les vieux ne lèvent presque pas.
