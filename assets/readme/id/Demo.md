# Seduhan Pour-Over yang Sempurna

Ritual pagi yang pelan dalam beberapa langkah. Tulis di **sumber** sebelah kiri atau langsung di *pratinjau* sebelah kanan: keduanya tetap sinkron, dan ==setiap pengeditan adalah perubahan yang bersih== pada Markdown.

> [!TIP]
> Air yang baru saja lepas dari titik didih, sekitar 94 °C, mengeluarkan rasa manis tanpa pahit.

## Rasio

Kebanyakan resep memakai satu bagian kopi untuk enam belas bagian air:

$$
m_{\text{air}} = 16 \times m_{\text{kopi}}
$$

Jadi $20\,\text{g}$ kopi memerlukan $320\,\text{g}$ air.

## Langkah demi langkah

```mermaid
flowchart LR
    A[Giling] --> B[Bloom] --> C[Tuang] --> D((Nikmati))
```

## Metode seduh

| Metode       | Gilingan     | Waktu |
|:-------------|:------------:|------:|
| Pour-over    | Medium-halus |  3.30 |
| French press | Kasar        |  4.00 |
| Espresso     | Halus        |  0.28 |

## Sebelum mulai

- [x] Biji kopi yang baru disangrai :coffee:
- [x] Penggiling burr
- [ ] Ketel berleher angsa

```python
def water_for(coffee_g: float, ratio: int = 16) -> float:
    return coffee_g * ratio
```

Bloom
: Siraman air pertama yang membuat bubuk kopi melepaskan gasnya.[^1]

[^1]: Biji yang segar paling banyak mengembang; yang lama nyaris tidak naik.
