<p align="center">
  <img src="assets/brand/lyrnova-logo-original.png" alt="Lyrnova" width="760">
</p>

<p align="center">
  Cliente desktop multiplataforma, leve e seguro, construído com Rust e Tauri.
</p>

<p align="center">
  <strong>Em desenvolvimento inicial.</strong>
</p>

## Sobre

Lyrnova é um cliente desktop comunitário para acessar o ChatGPT com integração
nativa ao sistema operacional. O projeto prioriza builds auditáveis,
empacotamento nativo e uma separação rigorosa entre o conteúdo remoto e os
recursos locais do computador.

## Tecnologia

- Rust como núcleo nativo;
- Tauri 2 para as janelas e integrações de desktop;
- WebKitGTK no Linux e WebView2 no Windows;
- interface local confiável separada do conteúdo remoto;
- nenhuma API nativa exposta diretamente ao webview remoto.

## Plataformas planejadas

- Windows;
- Fedora;
- Debian;
- Ubuntu;
- openSUSE;
- Lyra OS;
- OpenBase.

Cada plataforma receberá um pacote próprio e usará, sempre que possível, seu
gerenciador nativo para instalação e atualizações.

## Estado do projeto

O projeto está na fase de definição da arquitetura e validação do webview. Os
primeiros testes cobrirão autenticação, upload e download de arquivos,
microfone, área de transferência e notificações.

## Independência

Lyrnova é um projeto comunitário independente. Não é produzido, endossado nem
suportado pela OpenAI. ChatGPT é uma marca da OpenAI.
