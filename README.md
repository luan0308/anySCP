# Talktoall Files

Cliente de arquivos e terminal da talktoall para Windows: SSH, SFTP, FTP, FTPS e S3 em um app só, com um servidor MCP que deixa o **Claude** e o **Codex** trabalharem nos servidores salvos sem nunca ver as senhas.

## O que tem

- Servidores SSH (terminal + SFTP/SCP), FTP e FTPS (explícito e implícito), buckets S3.
- Navegador de arquivos com envio de pastas, edição no seu editor e fila de transferências.
- Senhas no cofre do sistema (Gerenciador de Credenciais do Windows).
- `talktoall-files-mcp`: servidor MCP instalado junto com o app. Em **Configurações › Claude e Codex**, um clique registra o servidor no Claude Desktop e no Codex. O Claude Code usa o comando mostrado na mesma tela.
- Interface em português, no visual da talktoall.

## Vindo do anySCP

Na primeira abertura, o Talktoall Files copia os servidores, grupos, comandos e configurações salvos pelo anySCP e passa a usar as mesmas senhas. A pasta do anySCP não é alterada. Ao conectar o Claude, a entrada antiga `anyscp` é substituída por `talktoall-files`.

## Desenvolvimento

```bash
pnpm install
pnpm tauri dev          # app
pnpm test               # testes do frontend
cd src-tauri && cargo test --lib
cd anyscp-mcp && cargo build --release   # servidor MCP
```

O instalador do Windows é gerado pelo GitHub Actions (`.github/workflows/release.yml`) a cada push na branch `release`.

## Créditos e licença

Baseado no [anySCP](https://github.com/macnev2013/anySCP), de Nevil Macwan. Código aberto sob a licença MIT (veja [LICENSE](LICENSE)). O README original do anySCP está em [README.anyscp.md](README.anyscp.md).
