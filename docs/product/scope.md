# Escopo do Lyrnova

Status: redefinido para o marco v0.1.0 em 2026-09-01.

## Visão

Lyrnova é um IDE desktop extensível. O usuário cria ou abre um projeto local,
navega e edita arquivos, executa e depura programas, usa Git e terminal e
instala plugins de linguagem, runtime, framework, ferramenta ou IA conforme a
necessidade.

O projeto tem interface, núcleo Rust, políticas de segurança e builds próprios.
Produtos como Codex e VS Code são referências de fluxo e familiaridade, não
fontes de código, assets ou identidade visual.

## Objetivos

- criar, abrir, organizar e restaurar projetos locais;
- editar código com abas, syntax, autocomplete, diagnósticos e navegação;
- permitir operações seguras de arquivos e pastas no workspace;
- apresentar Git status, diff, stage, commit e review;
- oferecer terminal PTY, tasks, testes, debug e worktrees;
- instalar, ativar, desativar, atualizar e desinstalar plugins;
- suportar LSP, DAP, templates e toolchains por APIs de plugin;
- permitir providers de IA opcionais sem acoplamento ao núcleo;
- funcionar primeiro em Linux e depois no Windows;
- produzir builds auditáveis para OBS e pacotes nativos.

## Não objetivos do primeiro ciclo

- incorporar ou navegar pela interface web do ChatGPT;
- copiar interface, marca, código ou assets proprietários do Codex;
- oferecer compatibilidade binária com extensões do VS Code;
- executar comandos ou alterações sem política visível;
- armazenar tokens/API keys em texto puro;
- implementar formulário próprio para senha da conta OpenAI;
- instalar Codex ou qualquer provider de IA por padrão;
- executar plugins sem manifesto, isolamento e permissões;
- suportar todas as linguagens, providers e extensões na v0.1;
- prometer trabalho cloud, colaboração ou handoff remoto;
- publicar pacotes antes do MVP, threat model e revisão de segurança.

## Fluxos essenciais

1. Criar um projeto por template ou abrir uma pasta local.
2. Navegar por arquivos, editar código e salvar em abas familiares.
3. Instalar um plugin de linguagem e receber LSP, tasks e templates.
4. Executar/testar o projeto no terminal e navegar pelos diagnósticos.
5. Iniciar uma sessão de debug e inspecionar o estado do programa.
6. Revisar Git diff, preparar arquivos e criar commit local.
7. Ativar ou remover plugins e compreender suas permissões.
8. Reiniciar o aplicativo e recuperar projeto, abas, painéis e plugins.

## Marcos

- **v0.1**: fundação do IDE, segurança e contrato da plataforma de plugins.
- **v0.2**: MVP do IDE extensível com projetos, editor, arquivos e plugins.
- **v0.3**: linguagens oficiais, LSP/DAP, terminal, Git e fluxos profissionais.
- **v0.4**: distribuição Linux/Windows e supply chain verificável.
- **v1.0**: segurança auditada, acessibilidade, evals e release estável.

## Identidade

- produto: Lyrnova;
- executable/package: `lyrnova`;
- application ID: `io.github.w3ti.lyrnova`;
- repositório: `https://github.com/w3ti/Lyrnova`;
- licença do código autoral: GPL-3.0-only.

Lyrnova é um projeto comunitário independente e não é produzido, endossado ou
suportado pela OpenAI, Google ou pelos mantenedores das tecnologias oferecidas
como plugins.
