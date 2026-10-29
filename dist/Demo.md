# The Perfect Pour-Over

A slow morning ritual in a few steps. Write it in the **source** on the left or straight in the *preview* on the right: both stay in sync, and ==every edit is a clean change== to the Markdown.

> [!TIP]
> Water just off the boil, around 94 °C, brings out sweetness without bitterness.

## The ratio

Most recipes settle on one part coffee to sixteen parts water:

$$
m_{\text{water}} = 16 \times m_{\text{coffee}}
$$

So $20\,\text{g}$ of coffee wants $320\,\text{g}$ of water.

## Step by step

```mermaid
flowchart LR
    A[Grind] --> B[Bloom] --> C[Pour] --> D((Enjoy))
```

## Brew methods

| Method       | Grind       |  Time |
|:-------------|:-----------:|------:|
| Pour-over    | Medium-fine |  3:30 |
| French press | Coarse      |  4:00 |
| Espresso     | Fine        |  0:28 |

## Before you start

- [x] Freshly roasted beans :coffee:
- [x] A burr grinder
- [ ] A kettle with a gooseneck spout

```python
def water_for(coffee_g: float, ratio: int = 16) -> float:
    return coffee_g * ratio
```

Bloom
: The first splash of water that lets the grounds release their gas.[^1]

[^1]: Fresh beans bloom the most; old ones barely rise.
