# Validação de entrada e apresentação — 2026-09-27

As entregas 11–12 estão publicadas em `adc0b4c`, com
[CI aprovado](https://github.com/w3ti/Lyrnova/actions/runs/36316630535).
A entrega 13 amplia a validação local de uso diário. Os resultados abaixo são
automatizados; não representam homologação humana integral do MVP.

## Entrada e foco

`native_input.py` envia eventos XTest ao binário real em Xvfb privado. WebDriver
somente observa a interface. Uma janela GTK em outro processo copia e cola pelo
clipboard do SO; seu buffer e as notificações de mudança do clipboard servem como
evidência. Nenhum texto é inserido no Monaco ou xterm via API de teste.

| Cenário | Aceite | Estado local |
|---|---|---|
| Acentuação | Agudo, til e circunflexo compõem `é`, `ã`, `ô`; cedilha e arquivo UTF-8 exatos | Passou em escala 100% |
| Clipboard do editor | GTK → Monaco → GTK preserva acentos, emoji, grego, japonês e múltiplas linhas | Passou em escala 100% |
| Clipboard do terminal | Ctrl+Shift+V cola; Enter executa; mouse seleciona saída e Ctrl+Shift+C copia para GTK | Passou em escala 100% |
| Interrupção do terminal | Ctrl+C interrompe job; Ctrl+D encerra shell | Passou em escala 100% |
| Paleta | Tab/Shift+Tab permanecem no modal com foco visível; Ctrl+K repetido/Escape retornam ao editor | Passou em escala 100% |
| Transição de foco | Comando da paleta foca novo diálogo; atalhos globais respeitam modal; fechar terminal retorna ao editor | Passou em escala 100% |
| Escala GTK 200% | Mesma jornada com tela física ampliada, mantendo fonte base de 16 CSS px e sem overflow horizontal da página | Passou em 2026-10-02 (DPR 2,083 no Xvfb, sem elementos além da largura do layout) |
| Wayland nativo e escala fracionária 150% | Repetir edição, clipboard, foco e diálogos no compositor real; conferir cursor, ícones e alinhamento | Pendente |
| IME completo | IBus/Fcitx: iniciar/cancelar composição e escolher candidatos sem perder texto ou disparar atalhos | Pendente |
| Leitor de tela | Conferir nomes, anúncio de estado/erro, navegação e leitura no editor e terminal | Pendente |
| Arrastar/soltar e múltiplos monitores | Conferir comportamento entre aplicações e ao mudar escala/monitor | Pendente |

O mapa de quatro teclas alterado pelo runner pertence somente à tela descartável.
Digitar teclas mortas não homologa todos os layouts; colar japonês não homologa
entrada por IME. Escala GTK inteira em Xvfb não equivale à escala fracionária do
compositor, e ausência de overflow não certifica toda a apresentação visual.

## Correções e evidência

- A paleta passou de uma camada com `aria-modal` para `<dialog>` modal, preservando
  o foco de origem e impedindo navegação para controles atrás dela.
- Comandos fecham a paleta antes de abrir/focar seu destino. Atalhos globais
  respeitam outros modais e eventos em composição.
- Ocultar o terminal focado devolve o foco ao editor, preservando o shell.
- Ctrl+Shift+C aciona a cópia do WebKit a partir do gesto de teclado; o listener
  do xterm fornece a seleção. Ctrl+C e o bloqueio de OSC 52 permanecem intactos.

Evidência local: `target/e2e-desktop/report.json` (11 grupos) e
`target/e2e-dialog-regression/report.json` (33 grupos, incluindo Rust).
Os 36 testes JavaScript, sintaxe e builds frontend/nativo passaram. A entrega
13 ainda não foi publicada nem validada no CI remoto.

Em 2026-10-02 a versão final do harness foi revalidada. A falha anterior em 200%
(`Timed out: external GTK clipboard peer`) vinha do peer, que fixava apenas
`Gtk 3.0`; com o typelib do GTK 4 instalado, `Gdk` sem versão carregava 4.0 e a
importação falhava antes de abrir a janela. O peer agora fixa `Gdk 3.0`, e seu
log é anexado ao relatório (`clipboardPeerLog`), pois a fixture é descartada.

A verificação de overflow comparava `scrollWidth` (arredondado para cima) com
`innerWidth` (arredondado para baixo). O Xvfb expõe DPR fracionário (1,042 em
100%, 2,083 em 200%), com layout de 1382,4 CSS px, então a métrica acusava 1 px
inexistente. Ela passou a usar a largura fracionária do layout e lista os
elementos que a ultrapassam; nenhum foi encontrado.

Com isso, os 11 grupos passaram em 100% (`target/e2e-desktop/report.json`,
início `2026-10-02T14:36:47Z`) e em 200% (`target/e2e-desktop-2x/report.json`,
início `2026-10-02T14:36:12Z`). A captura de 200% foi revisada sem cortes. A
regressão completa de 33 grupos não foi repetida; apenas o harness mudou.
