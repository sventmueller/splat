<p><img src="../../readme-icon.png" alt="Splat uygulama simgesi" width="168" height="168">&nbsp;&nbsp;&nbsp;<img src="screenshot.png" alt="Demo.md dosyasını açan Splat: solda Markdown kaynağı, sağda düzenlenebilir önizleme" width="274" height="168"></p>

# Splat

**Kaynakta ya da doğrudan önizlemede yazmanızı sağlayan macOS Markdown düzenleyicisi.**

Splat solda sözdizimi renkli Markdown kaynağınızı, sağda canlı ve tümüyle düzenlenebilir bir önizlemeyi gösterir. İkisinden birinde yazın: önizlemede yazdığınız her şey kaynakta asgari bir değişikliğe dönüşür; böylece dokunmadığınız satırlar bayt bayt aynı kalır ve Git diff’leriniz temiz kalır. Splat zaten kullandığınız Markdown’ı okur: CommonMark ve GitHub Flavored Markdown, GitHub uyarıları, Obsidian wiki bağlantıları, gömmeleri ve bilgi kutuları, Pandoc ve Typora eklentileri, gömülü HTML, LaTeX formülleri ile Mermaid, Graphviz ve PlantUML diyagramları; hepsi çevrimdışı görüntülenir. Bütün bir not klasörüne göz atın ve içinde arama yapın, son Git commit’inden beri neyin değiştiğini görün, Word’den ya da bir web sayfasından Markdown olarak yapıştırın ve tek tıkla PDF, HTML ya da Word olarak dışa aktarın. Rust ile gerçek AppKit denetimleri üzerine yazılmıştır, 35 dilde kullanılabilir ve siz istemedikçe ağa hiçbir şey göndermez.
 


[![Splat’ı macOS için indirin](https://img.shields.io/badge/Download-Splat_for_macOS-0A84FF?style=for-the-badge&logo=apple&logoColor=white)](https://github.com/sventmueller/splat/releases/latest/download/Splat.dmg)

**[Splat.dmg dosyasını indirin](https://github.com/sventmueller/splat/releases/latest/download/Splat.dmg)**: Apple tarafından imzalanmış ve notarize edilmiştir; Apple Silicon ve Intel üzerinde macOS 14 veya üstü için.

**Bir hata mı buldunuz?** Splat, dokunmadığınız satırları bayt bayt aynı tutar. Yine de biri değişirse veya bir dosya yanlış görüntülenirse [bir issue açın](https://github.com/sventmueller/splat/issues/new/choose) ve dosyayı ekleyin. Splat işinize yarıyorsa [GitHub’da bir yıldız](https://github.com/sventmueller/splat) başkalarının da onu bulmasına yardımcı olur.
 
 

[![Rust 2024 sürümü](https://img.shields.io/badge/Rust-2024_edition-000000?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![macOS 14 veya üstü](https://img.shields.io/badge/macOS-14%2B-000000?logo=apple&logoColor=white)](https://www.apple.com/macos/)
![Universal: Apple Silicon ve Intel](https://img.shields.io/badge/Universal-Apple_Silicon_%7C_Intel-333333?logo=apple&logoColor=white)
[![objc2 üzerinden AppKit](https://img.shields.io/badge/AppKit-objc2_0.6-147EFB?logo=apple&logoColor=white)](https://github.com/madsmtm/objc2)
[![WebKit](https://img.shields.io/badge/WebKit-offline-0A84FF?logo=safari&logoColor=white)](https://webkit.org)
[![Comrak 0.59](https://img.shields.io/badge/comrak-0.59-DEA584?logo=markdown&logoColor=white)](https://github.com/kivikakk/comrak)
[![CommonMark ve GFM](https://img.shields.io/badge/CommonMark-GFM-24292F?logo=github&logoColor=white)](https://github.github.com/gfm/)
[![MathJax 4.1.3](https://img.shields.io/badge/MathJax-4.1.3-1E6E9E?logo=latex&logoColor=white)](https://www.mathjax.org)
[![MathLive 0.111.4](https://img.shields.io/badge/MathLive-0.111.4-5A3FC0)](https://cortexjs.io/mathlive/)
[![Mermaid 12.3.0](https://img.shields.io/badge/Mermaid-12.3.0-FF3670?logo=mermaid&logoColor=white)](https://mermaid.js.org)
[![Viz.js 3.24 üzerinden Graphviz](https://img.shields.io/badge/Graphviz-Viz.js_3.24-2596BE)](https://graphviz.org)
[![PlantUML 1.2026.9](https://img.shields.io/badge/PlantUML-1.2026.9-A80036)](https://plantuml.com)
![35 arayüz dili](https://img.shields.io/badge/languages-35-4C9A2A)
![Varsayılan olarak çevrimdışı](https://img.shields.io/badge/network-offline_by_default-4C9A2A)
![Hardened Runtime ile imzalı](https://img.shields.io/badge/Hardened_Runtime-signed-4C9A2A)
[![Lisans: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT_OR_Apache--2.0-4C9A2A)](#lisans)

[Bahasa Indonesia](../id/README.md) ·
[Bahasa Melayu](../ms/README.md) ·
[Català](../ca/README.md) ·
[Čeština](../cs/README.md) ·
[Dansk](../da/README.md) ·
[Deutsch](../de/README.md) ·
[English](../../../README.md) ·
[Español](../es/README.md) ·
[Français](../fr/README.md) ·
[Hrvatski](../hr/README.md) ·
[Italiano](../it/README.md) ·
[Magyar](../hu/README.md) ·
[Nederlands](../nl/README.md) ·
[Norsk](../no/README.md) ·
[Polski](../pl/README.md) ·
[Português (Brasil)](../pt_BR/README.md) ·
[Português (Portugal)](../pt_PT/README.md) ·
[Română](../ro/README.md) ·
[Slovenčina](../sk/README.md) ·
[Slovenščina](../sl/README.md) ·
[Suomi](../fi/README.md) ·
[Svenska](../sv/README.md) ·
[Tiếng Việt](../vi/README.md) ·
**Türkçe** ·
[Ελληνικά](../el/README.md) ·
[Русский](../ru/README.md) ·
[Українська](../uk/README.md) ·
[עברית](../he/README.md)‎ ·
[العربية](../ar/README.md)‎ ·
[हिन्दी](../hi/README.md) ·
[ไทย](../th/README.md) ·
[한국어](../ko/README.md) ·
[日本語](../ja/README.md) ·
[简体中文](../zh-Hans/README.md) ·
[繁體中文](../zh-Hant/README.md)

## İçindekiler

- [Bir bakışta özellikler](#bir-bakışta-özellikler)
- [Gereksinimler](#gereksinimler)
- [Kurulum](#kurulum)
- [Başlarken](#başlarken)
- [Yazmak](#yazmak)
  - [Kaynakta](#kaynakta)
  - [Önizlemede](#önizlemede)
  - [Biçimlendirme komutları](#biçimlendirme-komutları)
  - [Listeler ve görevler](#listeler-ve-görevler)
  - [Tablolar](#tablolar)
  - [Biçimlendirilmiş metni yapıştırma](#biçimlendirilmiş-metni-yapıştırma)
  - [Meta veriler](#meta-veriler)
- [Desteklenen Markdown](#desteklenen-markdown)
  - [CommonMark ve GitHub Flavored Markdown](#commonmark-ve-github-flavored-markdown)
  - [Bilgi kutuları](#bilgi-kutuları)
  - [Genişletilmiş sözdizimi](#genişletilmiş-sözdizimi)
  - [Obsidian](#obsidian)
  - [Atıflar ve başvurular](#atıflar-ve-başvurular)
  - [Gömülü HTML](#gömülü-html)
  - [Tipografik noktalama](#tipografik-noktalama)
  - [Sözdizimi özeti](#sözdizimi-özeti)
- [Formüller](#formüller)
- [Diyagramlar](#diyagramlar)
- [Görüntüler](#görüntüler)
- [Bağlantılar ve gezinme](#bağlantılar-ve-gezinme)
- [Klasördeki notlar](#klasördeki-notlar)
- [Son commit’ten beri yapılan değişiklikler](#son-committen-beri-yapılan-değişiklikler)
- [Dışa aktarma, yazdırma ve paylaşma](#dışa-aktarma-yazdırma-ve-paylaşma)
- [Word belgelerini içe aktarma](#word-belgelerini-içe-aktarma)
- [Görünüm ve erişilebilirlik](#görünüm-ve-erişilebilirlik)
- [Diller](#diller)
- [Klavye kısayolları](#klavye-kısayolları)
- [Gizlilik ve güvenlik](#gizlilik-ve-güvenlik)
- [Büyük belgeler](#büyük-belgeler)
- [Bilinen sınırlamalar](#bilinen-sınırlamalar)
- [Sorun giderme](#sorun-giderme)
- [Geliştiriciler için](#geliştiriciler-için)
  - [Üçüncü taraf bileşenler](#üçüncü-taraf-bileşenler)
- [Lisans](#lisans)

## Bir bakışta özellikler

- **İki yazma yolu, tek metin.** Kaynak da önizleme de düzenlenebilir. Tek gerçek Markdown dosyasıdır: her belgenin tek bir geri alma geçmişi vardır ve önizlemedeki her düzenleme kaynakta olabilecek en küçük değişikliğe çevrilir.
- **Zaten kullandığınız Markdown.** CommonMark, GitHub Flavored Markdown, GitHub uyarıları, Obsidian, Pandoc ve Typora eklentileri, emoji kısa kodları, spoiler’lar, tanım listeleri, başlık çapaları ve GitHub README dosyalarındaki gibi HTML.
- **Formüller ve diyagramlar dahili.** MathJax ile LaTeX formülleri ve görsel bir formül düzenleyicisi. Mermaid, Graphviz ve PlantUML diyagramları Mac’inizde Java, sunucu ya da ağ bağlantısı olmadan görüntülenir.
- **Her şey için bir düğme.** Her Markdown yapısının bir araç çubuğu düğmesi ve bir menü öğesi vardır; yaygın olanların klavye kısayolu da bulunur. Tabloların satırlar, sütunlar ve hizalama için kendi menüsü vardır.
- **Yalnızca bir dosya değil, bir klasör.** Kenar çubuğu bir belgenin yanındaki notları listeler ve hepsinde arama yapar. Bir notun adını değiştirmek ona verilen bağlantıları da değiştirir; bunların çevresindeki her bayt olduğu gibi kalır.
- **Değişiklikler bir bakışta.** Bir Git deposunda, metnin yanındaki çubuklar son commit’ten farklı olanı iki bölmede de işaretler.
- **Word’den ve web’den Markdown’a.** Biçimlendirilmiş metni Markdown olarak, bir elektronik tablodaki hücreleri tablo olarak yapıştırın; Word belgelerini görüntüleriyle birlikte bütünüyle içe aktarın.
- **Hiçbir şey kaybetmeden dışa aktarma.** PDF, tek başına yeterli tek bir HTML dosyası ya da gerçek başlıklar, listeler, tablolar, dipnotlar ve görüntüler içeren bir Word belgesi. Sayfa numaraları, üst bilgiler ve başlık sayfası ekleyin ya da şirketinizin Word şablonunu kullanın.
- **Tepeden tırnağa yerel.** Gerçek AppKit pencereleri, sekmeler, menüler, açılır pencereler, Sürümler, Otomatik Kaydet, paylaşım menüsü, koyu mod, yazım denetimi, dikte ve VoiceOver. Electron yok, metnin kendisi için web görünümü yok.
- **Büyük dosyalarda hızlı.** 10 MB’lık bir belgede bir tuş vuruşunu işlemek, görünümün kendi işinden önce yaklaşık 10 ms sürer.
- **Gizliliğe duyarlı ve özenli.** Splat, App Sandbox içinde çalışır; Web’deki görüntüler yalnızca siz izin verdikten sonra yüklenir, diğer uygulamalara giden bağlantılar önce sorar ve güvenilmeyen görüntü verilerini çözen her şey korumalı, ayrı bir hizmette çalışır.
- **Kendi dilinizde.** Arayüz ve yerleşik Markdown hızlı başvurusu 35 dilde gelir: Arapça, Katalanca, Çince (Basitleştirilmiş), Çince (Geleneksel), Hırvatça, Çekçe, Danca, Felemenkçe, İngilizce, Fince, Fransızca, Almanca, Yunanca, İbranice, Hintçe, Macarca, Endonezce, İtalyanca, Japonca, Korece, Malayca, Norveççe, Lehçe, Portekizce (Brezilya), Portekizce (Portekiz), Romence, Rusça, Slovakça, Slovence, İspanyolca, İsveççe, Tayca, Türkçe, Ukraynaca ve Vietnamca.

## Gereksinimler

- macOS 14 Sonoma veya üstü. Splat macOS 27’de geliştirilir ve test edilir; orada Liquid Glass tasarımını kendiliğinden benimser.
- Apple Silicon veya Intel işlemcili bir Mac.
- Yaklaşık 45 MB disk alanı.

## Kurulum

**Disk görüntüsünden.** [`Splat.dmg`](https://github.com/sventmueller/splat/releases/latest/download/Splat.dmg) dosyasını indirin, açın ve Splat’ı Applications klasörüne sürükleyin. Görüntü bir Developer ID ile imzalanmış ve Apple tarafından notarize edilmiştir; bu yüzden uyarısız açılır.

**Homebrew ile.** Cask kendi tap’inde bulunur:

```sh
brew install --cask sventmueller/tap/splat
```

`brew upgrade --cask splat` yeni sürümleri kurar, `brew uninstall --zap --cask splat` ise Splat’ın ayarlarını ve önbelleklerini de kaldırır.

**Güncellemeler.** Disk görüntüsünden ya da Homebrew ile kurulan Splat kendini [Sparkle](https://sparkle-project.org) ile güncel tutar. İkinci açılışta, günde bir kez kendiliğinden güncelleme arayıp arayamayacağını sorar. **Splat › Güncellemeleri Denetle…** hemen denetler; aynı menüdeki **Güncellemeleri Otomatik Olarak Denetle** ve **Güncellemeleri Otomatik Olarak İndir ve Yükle** seçimlerinizi sonradan değiştirir. Splat bir güncellemeyi yalnızca geliştiricinin imzasını taşıyorsa kurar. Kendi derlemenizde güncelleyici yoktur.

**Kaynaktan.** Önkoşullar için [Building](../../../docs/development.md#building) bölümüne bakın, ardından:

```sh
make build
ditto target/release/Splat.app /Applications/Splat.app
open /Applications/Splat.app
```

Markdown dosyalarını çift tıklayarak Splat ile açmak için Finder’da herhangi bir `.md` dosyası seçin, **Dosya › Bilgi Ver**’ı seçin, **Şununla Aç** altında Splat’ı seçin ve **Tümünü Değiştir…**’e tıklayın. Splat kendini `.md`, `.markdown`, `.mdown` ve `.mkd` dosyaları için kaydeder.

## Başlarken

### Pencere

Her belge kendi penceresinde açılır; sekmeleri tercih ediyorsanız sekme olarak:

- **Sol: kaynak.** SF Mono yazı tipiyle, sözdizimi renkli Markdown’ınız.
- **Sağ: önizleme.** Görüntülenmiş belge, düzenlemeye hazır.
- **Kenar çubuğu (isteğe bağlı).** Tüm başlıklar bir ağaç olarak, birine tıklamak onu iki bölmede de gösterir: **Görüntü › Ana Hattı Göster** (⌃⌘S). İki sekme daha [klasördeki notları](#klasördeki-notlar) ve bunlar içinde aramayı barındırır. Pencere başlığının yanındaki düğme onu gösterir ve gizler; yeni pencereler onu son pencerenin bıraktığı gibi açar.
- **Araç çubuğu.** Düğmeler neyi değiştirdiklerine göre gruplanmıştır: karakterler, paragraflar ve eklenen öğeler; ardından önizlemeyi aç/kapat, Paylaş ve Yeni Belge. **Görüntü › Araç Çubuğunu Özelleştir…** ile özelleştirin.

Her iki tarafa daha fazla yer vermek için ayırıcıyı sürükleyin. **Görüntü › Önizlemeyi Gizle** (⌥⌘P) yalnızca kaynağı bırakır; Splat bunu pencere başına hatırlar. İki bölme birlikte kayar ve birinde yaptığınız seçim ötekinde vurgulanır.

Pencerenin köşesi metnin sözcük ve karakter sayısını ve okuma süresini, bir seçim varken aynısını seçim için de gösterir. Bağlantı adresleri, HTML etiketleri, meta veriler, yorumlar, formüller ve diyagramlar sayılmaz. Bu gösterge başta gizlidir; **Görüntü › Sözcük Sayısını Göster** onu açar.

### Belgeler, sekmeler ve sürümler

Splat sıradan bir macOS belge uygulamasıdır:

- **Otomatik Kaydet ve Sürümler.** Değişiklikler yerinde kaydedilir. **Dosya › Geri Dön › Tüm Sürümlere Göz At…** sizi zamanda geriye götürür.
- **Başka programlardan gelen değişiklikler.** Git, bir betik ya da başka bir düzenleyici açık bir belgeyi değiştirdiğinde Splat onu yeniden yükler; imleci ve kaydırma konumunu korur. Kendi kaydedilmemiş değişiklikleriniz varsa Splat bunları tutar ve macOS diğer sürümün üzerine kaydetmeden önce sorar.
- **Olağan dosya komutları.** Çoğalt (⇧⌘S), Adını Değiştir…, Şuraya Taşı… ve **Dosya › Son Kullanılanı Aç**.
- **Sekmeler.** **Dosya › Yeni Sekme** ya da araç çubuğundaki **+** düğmesi yeni bir belgeyi sekmede açar; standart Pencere menüsü sekmeleri birleştirir, taşır ve arasında geçiş yapar.
- **Pencere yerleşimi.** Son pencerenizin boyutu ve konumu, yeniden başlatmalar boyunca da hatırlanır. Orada zaten bir pencere varsa yenisi biraz kaydırılmış olarak açılır.
- **Dock simgesi.** Son pencereyi kapattıktan sonra simgeye tıklamak yeni bir belge açar.
- **Markdown dosyalarını Splat ile açma.** **Splat › Markdown İçin Saptanmış Uygulama Olarak Ayarla**, bir `.md` ya da `.markdown` dosyasına çift tıklamanın Splat’ı açmasını sağlar; macOS bir kez onay ister. Splat saptanmış uygulamayken öğenin yanında onay işareti görünür.

Dosyalar UTF-8 metin ya da bayt sırası işaretli UTF-16 olmalıdır. Splat başka kodlamaları tahmin edip metni bozmak yerine reddeder. Bayt sırası işareti kalır, UTF-16 UTF-16 olarak kalır ve tüm satırları CRLF ile biten bir dosyada eklediğiniz satırlar da CRLF alır.

**Bir çökmeden sonra** en fazla son birkaç saniyelik yazı kaybolur: Splat her beş saniyede bir kaydedilmemiş metni `~/Library/Application Support/Splat/Unsaved` klasörüne kopyalar, belge kaydedilince ya da Splat normal şekilde kapanınca kopyaları siler. Sonraki açılışta çökmenin orada bıraktıklarını `~/Library/Application Support/Splat/Recovery` klasörüne taşır ve bunu klasörü açan bir düğmeyle bir kez bildirir. Splat’ın hâlâ tepki verebildiği bir çökme, açık her belgenin metnini de oraya yazar.

### Hızlı başvuru

**Yardım › Markdown Hızlı Başvuru**, kendi dilinizdeki bir kopya kâğıdını yeni, başlıksız bir belge olarak açar. Sıradan bir belgedir; her şeyi hemen orada, kaynakta ya da önizlemede deneyin.

## Yazmak

### Kaynakta

Kaynak, Markdown için ayarlanmış düz metin düzenleyicisidir:

- **Sözdizimi renkleri** VS Code’daki gibi: başlıklar, vurgu, bağlantılar, kod, tablolar, HTML, front matter ve formüllerle diyagramların içinde LaTeX ile Mermaid. Açık, koyu ve Karşıtlığı Artır için paletler vardır.
- **Çitli kod blokları** Bash, C, C++, C#, CSS (SCSS ve Less dahil), Go, HTML, XML, Java, JavaScript, JSON, Kotlin, PHP, Python, Ruby, Rust, SQL, Swift, TypeScript, YAML, TeX ve Mermaid için renklendirilir; `js`, `py` ya da `sh` gibi olağan kısa adlar da geçerlidir.
- **Sürpriz değişimler yok.** Akıllı tırnaklar, akıllı tireler ve metin değişimi varsayılan olarak kapalıdır, çünkü `--`, `---` ve `"` işaretlerini bozarlar. **Düzen › Değişimler** altından açabilirsiniz.
- **Sistemin yazı araçları.** Yazım ve dilbilgisi denetimi, dikte, Bul ve Değiştir (⌥⌘F) ve Konuşma.
- **Sürükle ve bırak.** Bir görüntü dosyasını bırakırsanız `![ad](relative/path.png)` eklenir, başka bir dosya bırakırsanız ona bir bağlantı, bir URL bırakırsanız `[url](url)` eklenir. Her bırakma tek bir geri alma adımıdır.
- **Görüntü yapıştırma.** Panodaki bir resim, örneğin bir ekran görüntüsü, belgenin yanına PNG dosyası olarak kaydedilir (`Görüntü 2026-10-29 13.09.07.png`) ve görüntü olarak eklenir. Bunun için belgenin bir kez kaydedilmiş olması gerekir.

### Önizlemede

Önizleme bir kelime işlemci gibi davranır, ama onda yaptığınız her şey Markdown olarak son bulur:

- **Yazma.** `*` ya da `#` gibi Markdown’da anlam taşıyan karakterler, yazıldığı gibi görünsün diye öncelenir (escape edilir).
- **Return** yeni bir paragraf ya da liste öğesi başlatır. **⌥Return** veya **⌃Return** satır sonu ekler.
- **İmleçte biçimlendirme.** İmleç bir sözcüğün sonundayken Kalın, İtalik ve diğer karakter biçimleri bundan sonra yazdıklarınıza uygulanır. Durdurmak için biçimi yeniden seçin. Sözcüğün içindeyken tüm sözcük biçimlendirilir.
- **Boş satırlar.** Boş bir satırda Return bir boş satır daha ekler, Backspace onu kaldırır. Kaynakta her ek boş satır iki boş satır daha demektir; diğer Markdown programları bunları normal paragraf aralığı olarak gösterir.
- **Geri Al** iki bölmede de çalışır. Geri alma öğelerinin adları vardır (“Kalın’ı Geri Al”), yazma her ardışık yazım için tek adımdır ve Geri Al ile Yinele’den sonra iki bölme de değişikliğe kayar.
- **Formüller ve diyagramlar** bitmiş grafikler olarak görünür. Birine tıklayın, imleci üzerine getirin ya da üzerinde Return’e basın; düzenleyicisi açılır ([Formülleri](#formüller) ve [Diyagramları](#diyagramlar) görün).
- **Gömülü HTML, gömülü notlar ve front matter** gösterilir, ancak kaynakta düzenlenir. Üzerlerine tıklamak imleci kaynakta karşılık gelen yere koyar.

Markdown’ın ifade edemediği her şey önizlemede engellenir: Font ve Renk panelleri, cetvel, zengin metin ve görüntülerin yapıştırılması ya da bırakılması. Yapıştırılan HTML ve RTF, [Markdown Olarak Yapıştır](#biçimlendirilmiş-metni-yapıştırma) komutunu seçmediğiniz sürece düz metin olarak gelir.

### Biçimlendirme komutları

Her komuta araç çubuğundan, menü çubuğundan ve iki bölmenin bağlam menüsünden ulaşılır. Menü öğeleri, imlecin bulunduğu yerdeki biçimlendirme için onay işareti gösterir.

| Menü | Komutlar |
|---|---|
| **Biçim › Font** | Kalın, İtalik, Altı Çizgili, Üstü Çizgili, Vurgula, Kod, Üs, İndis |
| **Biçim › Başlık** | Gövde Metni, Başlık 1–6 |
| **Biçim › Liste** | Madde İşaretli Liste, Numaralı Liste, Görev Listesi |
| **Biçim** | Alıntı, Kod Bloku (Dil Yok ya da 20 dilden biri; mevcut bir bloğun dilini de değiştirir), Bilgi Kutusu (Not, İpucu, Önemli, Uyarı, Dikkat; aynı tür yeniden seçilirse kaldırılır), Formülü veya Diyagramı Düzenle…, Biçimlendirmeyi Temizle |
| **Ekle** | Bağlantı…, Görüntü…, Tablo…, Formül, Diyagram (Akış Şeması, Sıralama Diyagramı, Gantt Şeması), İçindekiler, Dipnot, Bölücü Çizgi |
| **Tablo** | Üste/Alta Satır Ekle, Sola/Sağa Sütun Ekle, Satırı Sil, Sütunu Sil, Hizalama (Sol, Ortala, Sağ, Saptanmış), Tablo Sütunlarını Hizala |
| **Görüntü** | Önizlemeyi Gizle/Göster, Ana Hattı Göster/Gizle, Tipografik noktalama, Büyüt/Küçült, Gerçek Boyut, araç çubuğu, sekme çubuğu, tam ekran |

Altı çizgili, vurgu, üs ve indis `<ins>`, `<mark>`, `<sup>` ve `<sub>` olarak yazılır; GitHub bunları da gösterir. Splat GitHub’ın kaldırdığı `<u>` etiketini ve daha kısa `==vurgu==`, `^üs^` ve `~indis~` biçimlerini de okur. GitHub `==` ve `^` işaretlerini yazıldığı gibi, `~indis~` biçimini ise üstü çizili gösterir.

### Listeler ve görevler

- Return listeyi sürdürür; boş bir öğede Return listeyi bitirir.
- **Tab** ve **⇧Tab** önizlemede liste öğelerini girintiler ve girintiyi geri alır.
- Önizlemede bir görev onay kutusuna tıklamak `- [ ]` ile `- [x]` arasında geçiş yapar.
- **Biçim › Liste** seçili paragrafları listeye dönüştürür ya da listenin türünü değiştirir.

### Tablolar

- **Ekle › Tablo…** satır sayısını (başlık satırı dahil) ve sütun sayısını sorar.
- Önizlemede **Tab** sonraki hücreye, **⇧Tab** önceki hücreye, **Return** alttaki hücreye geçer. Son hücrede Tab bir satır ekler.
- **Tablo** menüsü satır ve sütun ekler, siler ve bir sütunun hizalamasını belirler (⌘{, ⌘|, ⌘}).
- **Tablo › Tablo Sütunlarını Hizala** kaynağı boşluklarla doldurur, böylece sütunlar hizalanır.

### Biçimlendirilmiş metni yapıştırma

**Düzen › Markdown Olarak Yapıştır** (⌃⌘V), iki bölmenin bağlam menüsünde de bulunur ve panodakini seçimin olduğu yerde, tek bir geri alma adımı olarak Markdown’a dönüştürür. ⌘V düz metin eklemeyi sürdürür.

- **Tarayıcıdan, Word’den, Google Docs’tan, Confluence’tan, Notlar’dan ya da Pages’ten:** başlıklar, kalın, italik, altı çizili, üstü çizili, üs ve indis, bağlantılar, iç içe yapılarıyla listeler, alıntılar, kod ve tablolar da gelir.
- **Elektronik tablodan:** hücreler, başlık satırı olan ve sayı sütunları sağa hizalı bir tabloya dönüşür. Sekme karakteri, noktalı virgül ya da virgülle ayrılmış metin de, her satırda aynı sayıda sütun varsa, tabloya dönüşür.
- Web adresi olan **görüntüler** bağlantı olarak kalır. Yapıştırılan HTML içindeki bir görüntü belgenin yanında bir dosya olur; bunun için belgenin bir kez kaydedilmiş olması gerekir. Word’ün geçici görüntü dosyaları geride kalır, alt metinleri korunur.
- **Bağlantılar** yalnızca `http`, `https`, `mailto`, göreli ve `#` adreslerini korur; başka her bağlantıdan yalnızca metni kalır.

### Meta veriler

**Biçim › Meta Verileri Düzenle…** ya da önizlemenin üstündeki meta veri satırına tıklamak, YAML front matter’ın alanlarını içeren bir form açar. Alanlar değiştirilebilir, eklenebilir ve kaldırılabilir. Liste içeren bir alan liste olarak kalır: değerleri virgüllerle art arda yazılır, virgül içeren bir değer tırnak içinde durur. `tags`, `aliases`, `keywords`, `categories`, `authors` ya da `cssclasses` adlı yeni bir alan da liste olur. İç içe alanlar ve çok satırlı metin orada görülebilir ve kaynakta düzenlenir.

Formun değiştirdiği şey tek bir geri alma adımıdır ve yalnızca değiştirdiğiniz alanların satırlarına dokunur: diğer tüm satırların yorumları, sırası, alışılmadık boşlukları, tırnakları ve satır sonları bayt bayt aynı kalır. Meta verisi olmayan bir belgenin başına bir blok eklenir.

## Desteklenen Markdown

Splat, CommonMark ve GFM ile uyumlu bir ayrıştırıcı olan [comrak](https://github.com/kivikakk/comrak) ile ayrıştırır ve eklentilerinin çoğunu açar. Aşağıdakilerin hepsi önizlemede ve tüm dışa aktarmalarda görüntülenir.

### CommonMark ve GitHub Flavored Markdown

- Başlıklar (`#` ile ve altı çizgili), paragraflar, satır sonları, vurgu, güçlü vurgu, satır içi kod, alıntı blokları, bölücü çizgiler.
- Madde işaretli, numaralı ve iç içe listeler; görev listeleri.
- Bağlantılar (satır içi, başvurulu ve otomatik), görüntüler, bağlantı başvuru tanımları.
- Sözdizimi renkli çitli ve girintili kod blokları.
- Sütun hizalamalı tablolar.
- `~~iki tilde~~` ile üstü çizili metin.
- Dipnotlar: `metin[^1]` ve belgenin herhangi bir yerinde `[^1]: Not.` Belirteçe tıklamak nota atlar.
- En üstte `---` satırları arasında YAML front matter. Önizleme bunu tek satırda özetler, örneğin “Meta Veriler: title, date, tags”.

### Bilgi kutuları

GitHub uyarıları simge ve başlıkla renkli kutular olarak görüntülenir:

```markdown
> [!NOTE]
> Kullanıcıların bilmesi gereken yararlı bilgi.

> [!WARNING]
> Hemen dikkat gerektiren kritik içerik.
```

Beş tür Not, İpucu, Önemli, Uyarı ve Dikkat’tir. **Biçim › Bilgi Kutusu**, imlecin bulunduğu paragrafı bir bilgi kutusuna dönüştürür.

Obsidian bilgi kutuları da çalışır; istediğiniz başlığı ve bir katlanma durumunu taşıyabilir:

```markdown
> [!tip]- Açmak için ▸ simgesine tıklayın
> Açılana kadar gizli.

> [!question]+ Varsayılan olarak açık
> `+` açık, `-` kapalı başlar.
```

Obsidian’ın diğer türleri beş rengin en yakınıyla gösterilir: `success`, `check`, `done` ve `hint` İpucu olarak; `question`, `help` ve `faq` Önemli olarak; `attention` Uyarı olarak; `failure`, `fail`, `missing`, `danger`, `error` ve `bug` Dikkat olarak; geri kalan her şey Not olarak.

### Genişletilmiş sözdizimi

| Özellik | Sözdizimi | Kaynağı |
|---|---|---|
| Vurgu | `==işaretli metin==` | Obsidian, Typora |
| Üs | `x^2^` | Pandoc, Typora |
| İndis | `H~2~O` | Pandoc, Typora |
| Emoji kısa kodu | `:rocket:` 🚀 olur | GitHub |
| Spoiler | `\|\|gizli metin\|\|`, okumak için seçin | Discord |
| Satır içi dipnot | `metin^[notun kendisi]` | Pandoc, Obsidian |
| Tanım listesi | `Terim`, boş satır, `: Açıklama` | PHP Markdown Extra, Pandoc |
| Başlık çapası | `## Kurulum {#setup}`, `[bkz.](#setup)` ile bağlantı | Pandoc, kramdown |

Tanım listeleri kaynakta şöyle görünür:

```markdown
Terim

: Terimin açıklaması.
```

Özel bir çapa yoksa her başlık, GitHub’daki gibi metninden türetilmiş bir çapa alır: `## Başlarken` başlığına `#başlarken` ile ulaşılır.

### Obsidian

- **Wiki bağlantıları.** `[[Not]]`, aynı klasördeki ya da altındaki bir klasördeki `Not.md` dosyasına bağlanır. `[[Not|Gösterilen metin]]` kendi metninizi gösterir, `[[Not#Başlık]]` notu açar. ⌘-tıklama notu Splat’ta açar.
- **Gömmeler.** `![[Not]]` başka notu bu notun içinde gösterir; onun içine gömülü notlar bağlantı olarak görünür. `![[diagram.png]]` bir görüntü gösterir. Obsidian’daki gibi gömmeler notları ve görüntüleri belgenin klasöründe ya da altında adlandırır, asla mutlak yolla değil; yalnızca Markdown dosyaları not olarak gösterilir.
- **Yorumlar.** `%%Yalnızca kaynakta%%` kaynakta görünür kalır, önizlemeden ve her dışa aktarmadan çıkarılır; hem satır içinde hem de kendi satırlarında.
- **Etiketler.** `#fikir` ya da `#proje/splat` vurgulanır. ⌘-tıklama etiketi belgede ve kenar çubuğunda tüm klasörde arar.
- **Katlanabilir bilgi kutuları.** [Bilgi kutuları](#bilgi-kutuları) bölümüne bakın.

### Atıflar ve başvurular

- **Pandoc atıfları.** `[@doe2020]` ya da `[@doe2020; @roe2021]`, “(Doe 2020; Roe 2021)” gibi okunur. Girişler, front matter’da `bibliography: refs.bib` olarak adlandırılan BibTeX dosyasından gelir. O yoksa Splat, belgenin yanındaki `references.bib`, `literatur.bib` ya da `bibliography.bib` dosyasını kullanır.
- **GitHub ve GitLab başvuruları.** Belge, `origin` uzak deposu github.com’da ya da bir GitLab sunucusunda olan bir Git deposunda duruyorsa `@ad`, `#12` (issue) ve `!7` (merge request, yalnızca GitLab) vurgulanır. ⌘-tıklama bunları tarayıcıda açar.

### Gömülü HTML

GitHub README dosyalarında bulduğunuz türden HTML, bir tarayıcı motoruyla değil, kendi okuyucusuyla görüntülenir:

- ortalanmış paragraflar ve bloklar (`<p align="center">`, `<div align="center">`)
- boyutlu görüntüler (`<img src="logo.png" width="120">`)
- bağlantılar, `<kbd>⌘</kbd>`, `<br>`, `<u>`, `<ins>`, `<mark>`, `<sup>`, `<sub>`
- listeler ve tablolar; sütunlara yayılan hücreler dahil (`colspan`)
- katlanabilir bölümler: `<details><summary>Daha fazla</summary>…</details>`, ▸ simgesine tıklayarak açılır

HTML etiketleri arasındaki Markdown, Markdown olarak görüntülenir. Betikler, stiller, çerçeveler ve formlar tümüyle dışarıda bırakılır; bağlantılar ve görüntüler yalnızca `http`, `https`, `mailto` ve göreli adresleri korur.

### Tipografik noktalama

**Görüntü › Tipografik noktalama**, düz tırnakları arayüz dilinizin tırnak işaretleri olarak, `--` işaretini kısa tire, `---` işaretini uzun tire ve `...` işaretini üç nokta olarak gösterir. Almancada `"Ja"` „Ja“ olur, Fransızcada «Oui», Japoncada 「はい」; Çince ve Japoncanın tırnakları boşluksuz dizdiği yerlerde de böyledir. `צה"ל` gibi İbranice kısaltmalar gershayim’lerini alır: צה״ל. Kaynak düz karakterleri korur; önizleme ve tüm dışa aktarmalar tipografik olanları gösterir. Ayar, siz açana kadar kapalıdır.

### Sözdizimi özeti

| Öğe | Markdown |
|---|---|
| **Kalın** | `**Kalın**` |
| *İtalik* | `*İtalik*` |
| Altı çizgili | `<ins>Altı çizgili</ins>` veya `<u>Altı çizgili</u>` |
| ~~Üstü çizgili~~ | `~~Üstü çizgili~~` |
| Vurgu | `<mark>Vurgu</mark>` veya `==Vurgu==` |
| `Kod` | `` `Kod` `` |
| x² | `x<sup>2</sup>` veya `x^2^` |
| H₂O | `H<sub>2</sub>O` veya `H~2~O` |
| [Bağlantı](https://example.com) | `[Bağlantı](https://example.com)` |
| Başlık | `# Başlık` ile `###### Başlık` arası |
| Alıntı | `> Alıntı` |
| Madde işaretli liste | `- Öğe` |
| Numaralı liste | `1. Öğe` |
| Görev | `- [ ] Açık` ve `- [x] Tamam` |
| Kod bloku | ```` ```rust ```` … ```` ``` ```` |
| Tablo | `\| A \| B \|` ve başlığın altında `\|---\|---\|` |
| Dipnot | `Metin[^1]` ve `[^1]: Not` |
| Bölücü çizgi | `---` |
| Görüntü | `![Alt metin](picture.png)` |
| Formül | `$E = mc^2$`, `$$ … $$` |
| Diyagram | ```` ```mermaid ````, ```` ```dot ````, ```` ```plantuml ```` |
| Bilgi kutusu | `> [!TIP]` |
| Wiki bağlantısı | `[[Not]]`, `[[Not\|Metin]]` |
| Gömme | `![[Not]]`, `![[image.png]]` |
| Yorum | `%%Yorum%%` |
| Etiket | `#etiket` |
| Atıf | `[@key]` |
| Spoiler | `\|\|Spoiler\|\|` |
| Emoji | `:tada:` |

## Formüller

Formüller, TeX ve AMS matematiğiyle [MathJax](https://www.mathjax.org) tarafından keskin vektör grafikleri olarak görüntülenir. `\text{…}` içindeki metin herhangi bir yazı sisteminde olabilir: MathJax’in fontlarında bulunmayanlar Mac’inizin fontlarından gelir; Arapça ve İbranice sağdan sola akar, emoji renkli görünür.

| Tür | Sözdizimi |
|---|---|
| Satır içi | `$E = mc^2$`, `` $`E = mc^2`$ `` (GitHub, GitLab), `\(E = mc^2\)` |
| Ayrı satırda | `$$` … `$$`, `\[` … `\]` ya da `math` ile işaretli bir kod bloku |

**Formülü düzenleme.** Düzenleyicisini bir açılır pencerede açmak için önizlemede bir formüle tıklayın, çift tıklayın, üzerinde Return’e basın ya da imleci üzerine getirin. **Ekle › Formül** yeni bir tane ekler ve düzenleyiciyi hemen açar.

- **Görsel ya da LaTeX.** En üstteki anahtar, görsel bir düzenleyici ([MathLive](https://cortexjs.io/mathlive/)) ile LaTeX kodu arasında geçiş yapar. Splat seçiminizi hatırlar.
- **Yapı taşları**: kesir, karekök, üs ve indis, sınırları olan toplam ve integral, limit, içerikle birlikte büyüyen ayraçlar, 2×2 matris, Yunan harfleri ile ilişkiler, işleçler ve oklar. Bunlar □ yer tutucuları bırakır ve **Tab** bir sonrakine atlar.
- **Canlı önizleme.** Değişiklikler siz yazarken belgede görünür. MathJax hataları kodun altındaki bir satırda gösterilir.
- **Onaylama ya da vazgeçme.** Dışarıya tıklamak değişikliği tek bir geri alma adımı olarak korur; **Esc** veya **Vazgeç** onu atar. Yeni ve boş bir formülden Esc ile çıkmak onu yeniden kaldırır.
- Bağlam menüsü, işaretçinin altındaki formül için **Formülü Düzenle…** seçeneğini sunar.

VoiceOver bir formülü LaTeX kodu olarak okur. Splat formülleri görüntüler, eksiksiz LaTeX belgelerini değil: paket, TikZ ve `\documentclass` yoktur.

## Diyagramlar

Diyagramlar Splat’ın içinde, çevrimdışı ve açık ile koyu bir varyantla görüntülenir.

| Motor | Kod bloku | Ne çizer |
|---|---|---|
| [Mermaid](https://mermaid.js.org) | ```` ```mermaid ```` | akış şemaları, sıralama, sınıf, durum, varlık ilişkisi, Gantt, pasta, zihin haritaları, zaman çizelgeleri ve diğer tüm Mermaid türleri |
| [Graphviz](https://graphviz.org) | ```` ```dot ```` veya ```` ```graphviz ```` | DOT dilinde çizgeler |
| [PlantUML](https://plantuml.com) | ```` ```plantuml ```` veya ```` ```puml ```` | UML diyagramları; Java olmadan çalışır |

```mermaid
flowchart LR
    A[Markdown] --> B[Önizleme]
    B --> C[PDF]
```

**Diyagramı düzenleme**, formülü düzenlemeye benzer: kodu bir açılır pencerede açmak için ona tıklayın; sözdizimi renkleri vardır, hata iletileri kodun altındaki bir satırda görünür. **Ekle › Diyagram**, bir akış şeması, bir sıralama diyagramı ya da bir Gantt şeması şablonuyla başlar. Yapı taşları en yaygın üç Mermaid türüne yardımcı olur:

- **Akış Şeması:** Düğüm ▾ (dikdörtgen, yuvarlatılmış, daire, eşkenar dörtgen, veritabanı), Bağlantı, Etiketli bağlantı, Grup, Yön ▾
- **Sıralama Diyagramı:** Katılımcı, İleti, Yanıt, Not, Döngü, Alternatif yollar
- **Gantt Şeması:** Bölüm, Görev, Kilometre taşı

Diğer diyagram türleri kod olarak düzenlenir. VoiceOver bir diyagramın başlığını ya da alternatif metnini okur.

Açılıştan sonraki ilk diyagram yaklaşık bir saniye sürer, çünkü görüntüleme motoru arka planda başlar. 10 saniyeden uzun süren bir diyagram hata gösterir ve görüntüleme motoru bir sonrakine yeniden başlar. PlantUML’in motoru (3 MB) yalnızca ilk PlantUML diyagramıyla yüklenir. Web’den bir PlantUML `!include` engellenir.

## Görüntüler

- **Yerel görüntüler** belgeye göreli yollar kullanır: `![Logo](images/logo.png)`. Splat PNG, JPEG, GIF, WebP, BMP, SVG, HEIC, TIFF, AVIF ve PDF gösterir.
- **Web’deki görüntüler** siz izin verene kadar engelli kalır: önizlemenin üstündeki bir çubuk, bunları yüklemenin sunucularına belgeyi açtığınızı ve ne zaman açtığınızı bildireceğini açıklar. **Görüntüleri Yükle** bunlara bu belge için izin verir; Splat seçimi dosya başına hatırlar.
- **Boyut.** `<img width="200">` bir görüntünün boyutunu belirler; aksi halde görüntüler önizlemenin genişliğine sığar.
- **Diskteki değişiklikler.** Değiştirdiğiniz bir görüntü ya da belge adını verdikten sonra ortaya çıkan bir görüntü, belge yeniden açılmadan görünür. Ağ yüzünden başarısız olan web görüntüleri, bir belge onları gösterdiği sürece yeniden denenir.
- **Bellek.** Görüntüler en uzun kenarında 2560 piksele kadar gösterilir; bu bir A4 sayfayı da doldurur. Mac’in belleğinin sekizde birini doldurduklarında ve macOS bellek baskısı bildirdiği sürece daha küçük çözülürler.
- **Koyu mod.** Beyaz arka planlı görüntüler koyu önizlemede hafifçe karartılır.
- **Yapıştırma ve bırakma.** [Kaynakta](#kaynakta) bölümüne bakın.

## Bağlantılar ve gezinme

Önizlemede bir tıklama imleci yerleştirir, böylece bağlantı metnini düzenleyebilirsiniz; **⌘-tıklama** bağlantıyı izler:

- `http`, `https` ve `mailto` tarayıcınızda ya da posta uygulamanızda açılır.
- Markdown dosyaları Splat’ta açılır; diğer dosyalar ve klasörler Finder’da gösterilir.
- `#çapa` belgede o başlığa atlar.
- `slack://` ya da `vscode://` gibi başka bir uygulamayı açan adresler önce sorar ve tam adresi ile uygulamayı gösterir. **Vazgeç** önceden seçilidir.

**Ekle › İçindekiler**, tüm başlıkların bağlantılı bir listesini ekler. İmleç içindeyken aynı komut onu günceller. Ana hat kenar çubuğu (⌃⌘S) iki bölmede de herhangi bir başlığa atlar.

## Klasördeki notlar

Kenar çubuğunun üstündeki düğmeler ana hat, dosyalar ve arama arasında geçiş yapar.

- **Dosyalar** (**Görüntü › Dosyaları Göster**, ⌃⌘E). Belgenin klasöründeki ya da klasör bir Git deposundaysa deponun tamamındaki tüm Markdown dosyaları. Bir tıklama ya da Return bir notu bu pencerenin sekmesi olarak açar; zaten açıksa öne gelir. Klasör adının yanındaki düğme bu pencerede başka bir klasör gösterir. Gizli dosyalar, `node_modules`, `target`, `Pods`, `DerivedData`, `__pycache__` ve notu olmayan klasörler dışarıda kalır. Doğrudan Ana klasörünüzdeki bir belge yalnızca yanındaki notları gösterir; böylece Splat tüm klasörlerinizi dolaşıp her biri için sormaz.
- **Arama** (**Düzen › Bul › Tüm Notlarda Ara…**, ⇧⌘F). Büyük ya da küçük harf olmasına ve aksanlarının nasıl kodlandığına bakmadan tüm notlarda metin bulur. `#tag` bir etiket bulur, `#project` ayrıca `#project/splat` etiketini de bulur; kod içinde hiçbir şey bulmaz. Bir sonuca tıklamak notu o yerde açar.
- **Bir nota verilen bağlantılar.** Bir notun bağlam menüsü ona wiki bağlantısı, gömme, göreli bağlantı ya da bağlantı başvuru tanımıyla bağlanan notları listeler.
- **Notun adını değiştirme.** Aynı menü notun adını değiştirir ve bu bağlantıları günceller. Splat önce bunun kaç dosyada kaç bağlantı demek olduğunu söyler. Her bağlantıda yalnızca ad değişir ve dosyanın diğer her baytı aynı kalır. Açık belgeler değişikliği bir geri alma adımı olarak alır; diğer dosyalarda bunu yalnızca Git ya da Sürümler geri alabilir. Bu arada başka bir programın değiştirdiği bir dosya tüm bağlantıların ad değişikliğini durdurur ve Splat o dosyanın adını söyler.

Pencere öne geldiğinde liste klasörü izler. Arama dosyalara diskte oldukları haliyle bakar; henüz kaydetmediğiniz metin bir sonraki Otomatik Kaydet’ten itibaren sayılır.

## Son commit’ten beri yapılan değişiklikler

Bir Git deposundaki belgede, metnin yanındaki çubuklar son commit’ten farklı olanı kaynakta ve önizlemede işaretler: eklenen satırlar için yeşil, değişen satırlar için mavi, bir şeyin kaldırıldığı yerde kısa kırmızı bir işaret. Siz yazarken ve dalın konumu değiştiğinde güncellenirler. **Görüntü › Son Commit’ten Beri Yapılan Değişiklikler** bunları tüm pencereler için kapatır.

Yalnızca satır sonlarında farklı olan satırlar sayılmaz; bu yüzden LF’li bir commit karşısında CRLF’li bir dosya değişiklik göstermez. Yeni bir dosyada ilk commit’ine kadar işaret olmaz; 32 MB’ın üzerindeki bir belgede de olmaz.

## Dışa aktarma, yazdırma ve paylaşma

| Biçim | Nasıl | Ne elde edersiniz |
|---|---|---|
| **PDF** | **Dosya › PDF Olarak Dışa Aktar…** veya Paylaş menüsü | Kâğıt boyutunda sayfalar, her zaman açık görünümde; formüller ve diyagramlar vektör grafikleri olarak. **Dosya › Sayfa Düzeni…** kâğıdı belirler. |
| **Yazdırma** | **Dosya › Yazdır…** (⌘P) | PDF ile aynı düzen. |
| **HTML** | **Dosya › HTML Olarak Dışa Aktar…** veya Paylaş menüsü | Tek başına yeterli tek bir dosya: yerel görüntüler gömülü, formüller ve diyagramlar keskin görüntüler olarak, içindekiler çalışsın diye başlık çapaları. Betik yok. |
| **Word** | **Dosya › Word Belgesi Olarak Dışa Aktar…** veya Paylaş menüsü | Word’ün başlık stillerini, gerçek listeleri ve tabloları (birleştirilmiş hücreler dahil), Word dipnotları olarak dipnotları, gömülü görüntüleri, formülleri ve diyagramları içeren bir `.docx`. |
| **Markdown** | Araç çubuğundaki **Paylaş** düğmesi | Mail, Mesajlar, AirDrop ve diğer tüm paylaşım servisleri için belge bir `.md` dosyası olarak. |

**Dosya › Paylaş** menüsü bunların hepsini tek bir yerde sunar. Dışa aktarmalar tipografik noktalama ayarını ve web’den izin verdiğiniz görüntüleri kullanır; web’deki görüntüler HTML’de bağlantı olarak kalır.

**Dosya › Dışa Aktarma Seçenekleri…**, tüm belgeler için PDF’in, yazdırmanın ve Word’ün metne ne ekleyeceğini belirler:

- **Sayfa numaraları**, PDF ve yazdırmanın alt bilgisinde, `2 / 9` biçiminde.
- **Üst bilgi ve alt bilgi**, PDF ve yazdırma için. `{title}`, `{author}`, `{date}`, `{page}` ve `{pages}` kendi değerlerinin yerini tutar, ilk üçü belgenin meta verilerinden gelir.
- **Başlık sayfası**, PDF, yazdırma ve Word için, meta verilerdeki `title`, `subtitle`, `author` ve `date` ile ya da başlık olarak belgenin adıyla. Ne üst bilgisi ne de alt bilgisi vardır.
- **Word şablonu**, dışa aktarmanın görünümünü aldığı bir `.docx` ya da `.dotx`: stilleri (yerelleştirilmiş adlar dahil), teması, sayfa boyutu, kenar boşlukları, üst bilgileri ve alt bilgileri. Okunamayan bir şablon, size onsuz bir belge vermek yerine dışa aktarmayı durdurur ve bunu söyler.

## Word belgelerini içe aktarma

**Dosya › Word Belgesi İçe Aktar…**, bir `.docx` ya da `.dotx` dosyası ile Markdown’ının nereye gideceğini sorar, ardından sonucu açar. Görüntüler onun yanındaki, belgenin adına `-images` eklenerek adlandırılan bir klasöre gider.

Başlıklar ve listeler, stilleri yerelleştirilmiş adlara sahip belgelerde de Word’ün stillerinin ve numaralandırmasının tanımladığı gibi gelir. Kalın, italik, altı çizili, üstü çizili, vurgu, üs ve indis, bağlantılar, alıntılar, kod, dipnotlar, son notlar ve tablolar da gelir; sütunlara yayılan hücreler HTML tablosu olarak gelir. Formüller LaTeX olur. İzlenen değişiklikler kabul edilmiş sayılır; içindekiler tablosu dışarıda kalır, çünkü **Ekle › İçindekiler** yenisini oluşturur. Eski `.doc` dosyaları ve parolayla korunan belgeler reddedilir.

## Görünüm ve erişilebilirlik

- **Koyu mod** sistemi izler. Formüller ve diyagramlar her görünüm için ayrı görüntülenir.
- Sistem Ayarları’ndaki **Kontrastı artır**, sözdizimi renklerini yüksek karşıtlıklı paletlere geçirir.
- **Yakınlaştırma.** ⌘+, ⌘− ve ⌘0 iki bölmeyi de ölçekler.
- **Tam ekran, sekmeler ve özelleştirilebilir araç çubuğu**, her Mac uygulamasında olduğu gibi.
- **VoiceOver.** Bölmeler “Markdown kaynağı” ve “Önizleme” olarak etiketlenir, formüller kodları olarak, diyagramlar başlıkları olarak okunur. Her komuta klavyeden ulaşılabilir.

Splat’ın ayarlar penceresi yoktur: az sayıdaki seçenek uygulandıkları yerde, **Görüntü**, **Düzen › Değişimler** ve **Dosya › Dışa Aktarma Seçenekleri…** altında durur.

## Diller

Arayüz, menüler ve Markdown hızlı başvurusu 35 dile çevrilmiştir:

Arapça, Katalanca, Çince (Basitleştirilmiş), Çince (Geleneksel), Hırvatça, Çekçe, Danca, Felemenkçe, İngilizce, Fince, Fransızca, Almanca, Yunanca, İbranice, Hintçe, Macarca, Endonezce, İtalyanca, Japonca, Korece, Malayca, Norveççe, Lehçe, Portekizce (Brezilya), Portekizce (Portekiz), Romence, Rusça, Slovakça, Slovence, İspanyolca, İsveççe, Tayca, Türkçe, Ukraynaca ve Vietnamca.

Splat, **Sistem Ayarları › Genel › Dil ve Bölge** içindeki dil sırasını izler; orada **Uygulamalar** altından yalnızca Splat için de bir dil seçebilirsiniz. Dilinizde çeviri yoksa İngilizceyi kullanır. Menü terimleri Apple’ın kendi çevirilerini izler, böylece Splat Mac’inizin geri kalanı gibi okunur.

İbranice ve Arapça metin, arayüz dili ne olursa olsun göründüğü her yerde sağdan sola okunur: her paragraf, başlık ve liste öğesi ilk harfinin yönünü alır, alıntılar ve bilgi kutuları çubuklarını sağa çizer ve metni sağdan sola başlayan bir tablo sütunlarını sağdan sıralar. Kod blokları soldan sağa kalır. PDF, HTML ve Word dışa aktarmaları bu yönleri korur. Sistem dili İbranice ya da Arapça olduğunda macOS pencereyi de aynalar, böylece önizleme solda yer alır.

## Klavye kısayolları

| Eylem | Kısayol |
|---|---|
| Kalın, İtalik, Altı Çizgili | ⌘B, ⌘I, ⌘U |
| Üstü çizgili | ⇧⌘X |
| Kod | ⌥⇧⌘C |
| Bağlantı… | ⌘K |
| Gövde metni, Başlık 1–6 | ⌥⌘0, ⌥⌘1 ile ⌥⌘6 arası |
| Tablo sütununu sola, ortaya, sağa hizala | ⌘{, ⌘\|, ⌘} |
| Önizlemeyi gizle veya göster | ⌥⌘P |
| Ana hattı göster veya gizle | ⌃⌘S |
| Dosyaları göster veya gizle | ⌃⌘E |
| Tüm notlarda ara | ⇧⌘F |
| Markdown olarak yapıştır | ⌃⌘V |
| Büyüt, küçült, gerçek boyut | ⌘+, ⌘−, ⌘0 |
| Yeni belge, aç, kaydet, çoğalt | ⌘N, ⌘O, ⌘S, ⇧⌘S |
| Sayfa düzeni, yazdır | ⇧⌘P, ⌘P |
| Bul, bul ve değiştir | ⌘F, ⌥⌘F |
| Sonrakini, öncekini bul; seçimi kullan; seçime git | ⌘G, ⇧⌘G; ⌘E; ⌘J |
| Yazım ve dilbilgisi denetimini göster; belgeyi denetle | ⌘:, ⌘; |
| Yapıştır ve stili eşleştir | ⌥⇧⌘V |
| Araç çubuğunu gizle, tüm sekmeleri göster, tam ekran | ⌥⌘T, ⇧⌘\\, ⌃⌘F |
| Satır sonu (önizleme) | ⌥Return veya ⌃Return |
| Tabloda sonraki hücre / önceki hücre (önizleme) | Tab / ⇧Tab |
| Liste öğesini girintile / girintiyi geri al (önizleme) | Tab / ⇧Tab |
| Formül düzenleyicisinde sonraki yer tutucu | Tab |
| Formül veya diyagram düzenleyicisini değişiklik yapmadan kapat | Esc |
| Bir bağlantıyı, etiketi veya başvuruyu izle (önizleme) | ⌘-tıklama |

Splat, Apple’ın standart kısayollarına dokunmaz.

## Gizlilik ve güvenlik

Bir Markdown dosyası herkesten gelebilir; bu yüzden Splat her belgeyi güvenilmez sayar.

- **Varsayılan olarak ağ yok.** Splat’ta telemetri yoktur. Güncellemeleri yalnızca siz onay verdikten sonra ya da **Güncellemeleri Denetle…**’yi seçtiğinizde arar: o zaman GitHub’dan küçük bir akış dosyası yükler ve istek uygulamanın adı ile sürümünden başka bir şey içermez. Formüller ve diyagramlar yerelde görüntülenir; bunu yapan web görünümleri ağa erişemez. **Yardım › Sorun Bildir…**, tarayıcınızda GitHub’daki bir formu açar; adresi Splat sürümünü, macOS sürümünü ve Mac türünü içerir, belgelerinizden hiçbir şey içermez.
- **Web’deki görüntüler** yalnızca izninizden sonra, yalnızca `https` üzerinden, asla yerel ağınızdaki sunuculardan olmayarak, her biri en fazla 10 MB ve çerezsiz özel bir oturumda yüklenir.
- **Yalıtılmış çözme.** Görüntüler ile formüllerin ve diyagramların grafikleri ayrı bir hizmette çözülür; macOS bu hizmeti kendine ait bir sanal alanda (sandbox) başlatır: dosyalarınızın hiçbiri yok, ağ yok. Hizmet yalnızca boyutu denetlenen pikselleri geri verir ve bir çözme işlemi 10 saniyeden uzun sürdüğünde ya da çok fazla bellek kullandığında kendini sonlandırır. Apple’ın kendi görüntü çözücüleri yalnızca HEIC, TIFF, AVIF ve PDF’yi görür, o da yalnızca veri gerçekten o biçimdeyse. SVG’ler, formüller ve diyagramlar da bu hizmetten geçer; bu yüzden Splat’ta sabit çözünürlüklü resimlerdir: nokta başına dört piksel, uzun kenarda en fazla 2560.
- **Yerel dosyalar** bir belgeye yalnızca onun adlandırdığı yolla ve yalnızca erişim izni verdiğiniz bir klasörden ulaşır: Splat açtığınız bir belgeyi okuyabilir, ancak çevresindeki resimleri, notları, kaynakçayı ve depoyu ancak önizlemenin üstündeki çubukta **Klasöre Erişim İzni Ver…** ile klasörlerine erişim izni verdiğinizde okur. Verilen izin tüm alt klasörleri kapsar ve hatırlanır. Gömmeler Markdown notlarını belgenin klasöründen ve altından gösterir, eksik bir resim de yalnızca orada aranır. Başka yerdeki, mutlak ya da `~` ile başlayan bir görüntü yolu, erişim izni verilmiş bir klasördeyse önizlemede yüklenir; HTML ve Word dışa aktarmaları, yazdırma ve PDF’ler yalnızca belgenin klasöründe ya da Git deposunda (bu depo Ana klasörünüzü de içermediği sürece) olanı yanına alır, böylece başkasından gelen bir belge dosyalarınızı ilettiğiniz bir dışa aktarmaya çekemez.
- **Not klasörleri** yalnızca kenar çubuğu onları gösterirken okunur; bir notun adını değiştirmek, diğer dosyaları ancak kaç tane olduğunu onayladıktan sonra değiştirir.
- **Git depolarını** Splat’ın kendisi okur. Başkasından gelen bir depodaki yapılandırması başka programları başlatabilen `git` programını asla çalıştırmaz ve açık belgenin son commit’teki sürümünden fazlasını okumaz. Oraya hiçbir şey yazmaz ve ağa çıkmaz.
- **Word belgeleri ve yapıştırılan metin**, boyut, iç içe geçme ve bir dosyanın ne kadar açılabileceği için sınırlarla, Splat’ın kendi ayrıştırıcıları tarafından okunur. İçlerindeki bağlantılar yalnızca Splat’ın başka yerlerde izin verdiği adresleri korur; bir Word dosyasındaki görüntülerin adları nereye yazılacaklarını asla belirlemez.
- **Kaydedilmemiş metnin kopyaları**, bir çökmeden sonra kurtarma için Mac’inizde, Library klasörünüzde kalır. Kaydetmeyle ya da Splat’ın normal sonlanmasıyla giderler; bir çökmenin geride bıraktığı kopyalar siz silene kadar kurtarma klasöründe kalır.
- **Bağlantılar** bir tıklamayla asla bir program başlatmaz. Web ve posta bağlantıları olağan şekilde açılır; diğer uygulamalara giden bağlantılar önce sorar.
- **Belgelerdeki HTML**, Splat’ın kendi ayrıştırıcısı tarafından okunur. Betikler, stiller, çerçeveler ve formlar asla çalışmaz.
- **Dışa aktarmalar** betik taşımaz. HTML sayfaları, GitHub’ın yaptığı gibi yalnızca sabit bir zararsız etiket ve öznitelik listesini ve ek olarak bir içerik güvenlik politikasını korur. Word dosyaları, seçtiğiniz bir Word şablonunun beraberinde getirdiği bağlantılar dışında yalnızca `http`, `https` ve `mailto` adreslerine bağlanır.
- **Kod imzalama**, Hardened Runtime ve App Sandbox ile. Splat açtığınız belgeleri ve erişim izni verdiğiniz klasörleri okuyup yazabilir, izin verdiğiniz Web görüntüleri ve güncellemeler için ağa bağlanabilir ve yazdırabilir; başka hiçbir şey yapamaz.

## Büyük belgeler

Splat, birkaç megabaytlık belgelerde de duyarlı kalır:

- Kaynak, yalnızca görünen kısmı yerleştiren TextKit 2’yi kullanır.
- Önizlemede yazarken ve 2 MB’a kadar belgelerin kaynağında, yalnızca etkilenen paragraflar yeniden ayrıştırılır, renklendirilir ve yerleştirilir. 10 MB’lık bir belgede ayrıştırma ve renklendirme yaklaşık 3 ms, işleme yaklaşık 10 ms, önizlemesinde görünümün kendi işiyle birlikte tam bir tuş vuruşu yaklaşık 30 ms sürer.
- Büyük önizlemeler önce görünen kısmı, kalanını uygulama boştayken görüntüler. Görünüme kaydırılan bloklar çizilmeden önce görüntülenir.
- Daha büyük belgelerin kaynağında, siz yazarken önizleme için ayrıştırma arka planda çalışır.
- Belge yaklaşık 4 GB bellek gerektirecek olduğunda düz metin olarak kalır; bu, sıradan Markdown için yaklaşık 50 ila 120 MB, yalnızca tablo veya başlıklardan oluşan belgeler için ise yalnızca 10 ila 20 MB demektir: kaynakta renk yoktur ve önizleme neden boş kaldığını söyler. Dışa aktarmalar yine çalışır.
- Yaklaşık 200 KB’tan itibaren bir belge, ayrıştırılmadan önce penceresini açar; 5 MB’ta renkler ve önizleme yaklaşık çeyrek saniye sonra gelir.
- Formüller ve diyagramlar dahil tüm görüntüler birlikte, Mac’in belleğinin en fazla dörtte birini, bellek azken de bunun yarısını kaplar. Bunun ötesinde, diğer görüntüler yeniden yer açılana kadar yer tutucu olarak gösterilir.

Bazı değişiklikler tümüyle işlenir; 10 MB’ta tuş vuruşu başına yaklaşık 300 ms sürer: bir dipnotun tanımında ya da başvurularında, satır içi bir dipnotun yanında, belgenin en sonunda bir dipnotun yanında, front matter’da ya da bağlantı başvuru tanımlarında yapılanlar ve `%%` yorum blokları olan, iki kez ya da bir alıntı veya liste içinde tanımlanmış dipnotu olan ya da 1000’den fazla satır içi dipnotu veya kapatılmamış formül işareti olan bir belgedeki her değişiklik. Bellek dosyayla birlikte büyür: 1 MB’lık Markdown için kabaca 115–160 MB, 10 MB için 340–510 MB.

## Bilinen sınırlamalar

- **Tek tilde indis demektir.** `~metin~` Pandoc’taki gibi indistir; üstü çizili için `~~iki~~` gerekir. GitHub tek tildeyle de üstünü çizer.
- **Önizlemede salt okunur:** gömülü HTML ve gömülü notlar kaynakta, front matter ise kendi formunda ya da kaynakta düzenlenir.
- **Çok derin iç içe geçme:** 64 düzeyden derin iç içe HTML ve 16 düzeyden derin satır içi HTML içeriğine yer bırakır.
- **Birleştirilmiş tablo hücreleri** yalnızca HTML tablolarında ve yalnızca sütunlar boyunca vardır (`colspan`, `rowspan` yok). MultiMarkdown’ın birleştirilmiş hücreler için tablo sözdizimi desteklenmez.
- **Kaynakçalar** yalnızca BibTeX’ten okunur; CSL JSON ya da YAML’dan okunmaz.
- **Başvurular** github.com’da ya da bir GitLab sunucusunda bir depo gerektirir; `!7` yalnızca GitLab’da vardır.
- **Tipografik noktalama:** Almancadaki kapanış tek tırnağı kesme işaretinden ayırt edilemez ve ’ olarak dizilir.
- **Belgeler değil, formüller:** LaTeX paketi, TikZ ya da tam belge yok.
- **Görsel formül düzenleyicisi** LaTeX’i MathLive’ın gösteriminde yazar: boşluklar kaybolur (`E = mc^2` `E=mc^2` olur), görüntü aynı kalır.
- **Sözcük sayısı:** Tayca, Laoca, Kmerce ve Birmanca boşluksuz yazılır ve sözcük başına değil, metin parçası başına sayılır. `3.14` ve `snake_case` ikişer sözcüktür.
- **Notun adını değiştirmek** gömülü HTML’deki bağlantıları değiştirmez; yeni ad `#`, `|`, `^`, `[`, `]` ya da `\` içeriyorsa wiki bağlantısı olduğu gibi kalır.
- **Değişiklik işaretleri** SHA-1 nesneleri ve düz ya da paketlenmiş ref’ler içeren depoları okur; SHA-256 ya da reftable kullanan bir depo işaret göstermez. LFS gibi bir Git filtresi altındaki dosya, filtrenin sakladığıyla karşılaştırılır.
- **Word içe aktarma** üst bilgi, alt bilgi ve yorumlardaki metni geride bırakır; `.emz` ya da SVG biçimindeki görüntüler yalnızca alt metinlerini korur.
- **Markdown Olarak Yapıştır** RTF’yi Batı metni olarak okur ve orada başlıkları yalnızca yazı tipi boyutlarından alır. Çoğu program HTML de sunar ve bu komut onu tercih eder.
- **Tasarım gereği:** satır numarası yok, ayarlar penceresi yok, Help Book yok, App Store’da değil.
- **Intel’de daha az test edildi.** Pencere öz-testleri yalnızca Apple Silicon’da çalışır; Intel’de CI `cargo test` çalıştırır.

## Sorun giderme

**Splat’ı yeniden derledikten sonra son belgeler listesi boş.** macOS, son belgeleri ve klasör izinlerini uygulamanın kod imzasına bağlar. Yerel derlemeler, oturum açma anahtarlığınızdaki “Splat Local Signing” sertifikasıyla imzalanır, bu yüzden yeniden derlemelerde ikisi de korunur. Sertifika silindiyse sonraki derleme yenisini oluşturur ve listeler bir kez baştan başlar.

**Disk görüntüsündeki uygulama, kendi derlememden farklı son belgeler gösteriyor.** İkisi farklı imzalar taşır, bu yüzden macOS her biri için ayrı bir liste ve ayrı klasör izinleri tutar.

**HTML ya da başka sözdizimi düz kod olarak görünüyor.** Belgeyi Splat’ın eski bir kopyası açmış olabilir. Onu kapatın, Finder’da belge için **Dosya › Bilgi Ver**’ı seçin ve **Şununla Aç** altında Applications klasörünüzdeki Splat’ın yazılı olduğunu denetleyin.

**Splat menüsünde Güncellemeleri Denetle… yok.** Yalnızca disk görüntüsünden ya da Homebrew ile gelen sürüm kendini günceller; kendi derlemenizde güncelleyici yoktur.

**Web’deki görüntüler görünmüyor.** Önizlemenin üstündeki çubukta **Görüntüleri Yükle**’ye tıklayın. Yalnızca herkese açık sunuculardaki `https` görüntüleri yüklenir.

**Ekran görüntüsünü yapıştırmak önce kaydetmemi istiyor.** Yapıştırılan görüntüler belgenin yanına dosya olarak kaydedilir; bunun için diskte bir yer gerekir. Belgeyi kaydedin, sonra yeniden yapıştırın.

**“Dosya UTF-8 metni değil.”** Dosya başka bir kodlamadadır ve onu belirten bir bayt sırası işareti yoktur. Önce dönüştürün, örneğin `iconv -f ISO-8859-1 -t UTF-8 old.md > new.md` ile.

**Bir formül ya da diyagram hata gösteriyor.** Üzerine tıklayın: düzenleyici, kodun altında MathJax, Mermaid, Graphviz ya da PlantUML’in iletisini gösterir.

## Geliştiriciler için

Derleme, test, kodun yapısı ve uyduğu kurallar [docs/development.md](../../../docs/development.md) dosyasında anlatılıyor (İngilizce).

### Üçüncü taraf bileşenler

| Bileşen | Sürüm | Kullanım amacı | Lisans |
|---|---|---|---|
| [comrak](https://github.com/kivikakk/comrak) | 0.59, [bir satırı değiştirilmiş](../../../vendor/comrak/PATCH.md) | Markdown ayrıştırıcısı | BSD-2-Clause |
| [objc2](https://github.com/madsmtm/objc2), objc2-foundation, block2 | 0.6 / 0.3 | Objective-C çalışma zamanı, Foundation, bloklar | MIT |
| objc2-app-kit, objc2-web-kit, dispatch2 | 0.3 | AppKit, WebKit ve Grand Central Dispatch bağlayıcıları | Zlib, Apache-2.0 veya MIT |
| [krilla](https://github.com/LaurenzV/krilla), krilla-svg, [usvg](https://github.com/linebender/resvg) | 0.8 / 0.47 | formüller ve SVG görüntüleri vektör PDF olarak | MIT veya Apache-2.0 |
| [image](https://github.com/image-rs/image) | 0.25 | PNG, JPEG, GIF, WebP ve BMP çözme | MIT veya Apache-2.0 |
| [unicode-width](https://github.com/unicode-rs/unicode-width) | 0.2 | tablo sütunlarını hizalama | MIT veya Apache-2.0 |
| [entities](https://github.com/p-jackson/entities) | 1 | HTML karakter başvurularının adları | MIT |
| [miniz_oxide](https://github.com/Frommi/miniz_oxide) | 0.9 | Word dosyalarını ve Git nesnelerini açma | MIT, Zlib veya Apache-2.0 |
| [roxmltree](https://github.com/RazrFalcon/roxmltree) | 0.21 | Word dosyalarının XML’ini okuma | MIT veya Apache-2.0 |
| [encoding_rs](https://github.com/hsivonen/encoding_rs) | 0.8 | eski kod sayfalarıyla yazılmış RTF metnini okuma | MIT veya Apache-2.0, BSD-3-Clause |
| [unicode-normalization](https://github.com/unicode-rs/unicode-normalization) | 0.1 | birleşik ve ayrışık harfler arasında adlar ve arama | MIT veya Apache-2.0, Unicode-3.0 |
| [emojis](https://github.com/rossmacarthur/emojis) | 0.9 | içe aktarılan metinde `:shortcode:` benzeri metni olduğu gibi koruma | MIT veya Apache-2.0, Unicode-3.0 |
| [MathJax](https://www.mathjax.org) | 4.1.3 | formülleri görüntüleme | Apache-2.0 |
| [MathLive](https://cortexjs.io/mathlive/) | 0.111.4 | KaTeX fontlarıyla görsel formül düzenleyicisi | MIT |
| [Mermaid](https://mermaid.js.org) | 12.3.0 | Mermaid diyagramları | MIT |
| [Viz.js](https://github.com/mdaines/viz-js) | 3.24.0 | WebAssembly’ye derlenmiş Graphviz; Graphviz (EPL-1.0) ve Expat (MIT) içerir | MIT |
| [PlantUML](https://github.com/plantuml/plantuml) (`@plantuml/core`) | 1.2026.9 | JavaScript’e derlenmiş PlantUML diyagramları | MIT |
| [Sparkle](https://sparkle-project.org) | 2.10.1 | disk görüntüsü sürümünün güncellemeleri | MIT; parçaları BSD ve zlib lisansları altında |

JavaScript kütüphaneleri `resources/js/` içinde tam yayımlandıkları gibi saklanır; [vendor.lock](../../../vendor.lock) kaynaklarını ve SHA-256 sağlama toplamlarını kaydeder. Viz.js içindeki Graphviz ve Expat’ınkiler dahil lisans metinleri, yanlarında `resources/js/LICENSE-*` olarak durur ve uygulamanın içinde dağıtılır. Sparkle depoda saklanmaz: `cargo xtask dmg` sürümünü indirir ve xtask’taki sağlama toplamına karşı denetler; lisansı uygulamanın içinde `LICENSE-sparkle` olarak dağıtılır.

## Lisans

Splat, aşağıdakilerden herhangi birinin altında lisanslanmıştır

- Apache Lisansı, Sürüm 2.0 ([LICENSE-APACHE](../../../LICENSE-APACHE))
- MIT Lisansı ([LICENSE-MIT](../../../LICENSE-MIT))

tercihinize bağlı olarak.

Siz açıkça başka türlü belirtmedikçe, Apache-2.0 lisansında tanımlandığı üzere Splat’a dahil edilmek üzere sizin tarafınızdan kasıtlı olarak sunulan her katkı, ek hüküm ya da koşul olmaksızın yukarıdaki gibi çift lisanslı olacaktır.
