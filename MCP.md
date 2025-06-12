# MCP (Model Context Protocol) Integration with Tenx

This document outlines the implementation plan for integrating MCP servers with Tenx, allowing AI models to interact with external tools and services through the Model Context Protocol

## MCP Protocol Compliance

This document is up to date with the latest MCP protocol version: 2025-03-26.

This implementation should be compatible with MCP servers running different protocol versions:

- 2025-03-26
- 2024-11-05

## Overview

MCP integration will enable Tenx to:

- Connect to external MCP servers providing tools and resources
- Support STDIO and Streamable HTTP transport protocols
- Include tool definitions with comprehensive annotations in model prompts
- Parse and execute tool calls from model responses
- Support OAuth 2.1 authorization for secure access to remote MCP servers

## Implementation Plan

### Module 1: Server Configuration

1.1. **MCP Configuration File (`mcp.json`)**
- Located in users project directory
- Defines available MCP servers and their connection details
   ```json
   {
     "mcpServers": {
       "git": {
         "command": "npx",
         "args": ["-y", "@modelcontextprotocol/server-git", "--repository", "."],
         "env": {},
         "transport": "stdio"
       },
       "github": {
         "url": "https://api.github.com/mcp",
         "transport": "streamable-http",
         "authorization": {
           "type": "oauth2.1",
           "client_id": "your-client-id",
           "scopes": ["read:repo", "write:issues"]
         }
       }
     }
   }
   ```

### Module 2: MCP Client Implementation

2.1. **Core MCP Client**
- Protocol implementation
  - Option: A - Use an existing implementation of the MCP protocol
    - [rust-mcp-sdk](https://github.com/rust-mcp-stack/rust-mcp-sdk)
    - [rmcp](https://github.com/modelcontextprotocol/rust-sdk/tree/main/crates/rmcp)
    - [goose](https://github.com/block/goose/tree/main/crates/mcp-client)
    - [zed](https://github.com/zed-industries/zed/tree/f428d54b74611dbd5f58b4239b4ddd96eeef7e33/crates/context_server)
  - Option B - roll our own
    - Implement MCP JSON-RPC 2.0 entities
    - Implement transport layers
    - Support comprehensive authorization framework with OAuth 2.1
  - Process/Connection management
    - Handle server lifecycle (start, stop, health checks with progress notifications)
    - Reconnection logic with exponential backoff
    - Session management for HTTP transport with proper session handling

2.2. **Tool Listing**
- Query connected servers for available tools
  - Name
  - Description
  - Input Schema
- Cache tool definitions with refresh mechanisms
- Handle tools from multiple servers with conflict resolution
- Support comprehensive tool annotations:
  - Behavioral annotations (read-only, destructive, etc.)
  - Safety annotations for user consent flows
  - Performance annotations for optimization

2.3. **Authorization Framework**
- Implement OAuth 2.1 client with PKCE (mandatory)
- Support Dynamic Client Registration where available
- Handle authorization code flow for HTTP-based MCP servers
- Secure token storage and refresh mechanisms
- Integration with MCP server authorization endpoints
- Support for `.well-known/oauth-authorization-server` discovery

### Module 3: Model Integration

3.1. **Prompt Enhancement**
- Extend prompt include tool definitions with annotations/instructions
- Enhanced tool filtering based on annotations and context
- Make sure we are using prompt caching whenever available
- Optionally: use function calls for OpenAI

3.2. **Tool Call Parsing**
- Parse tool call requests from model responses
- Validate tool call parameters against schemas

3.3. **Tool Call Execution**
- Execute tool calls against appropriate MCP servers
- Enhanced error handling and timeout management
- Handle authorization requirements per tool
- Return formatted results initially for text, but eventually support other content types
- Progress tracking with descriptive status updates

### Module 4: Conversation Integration

4.1. **Multi-turn Tool Support**
- Extend conversation flow to handle multiple tool call/response cycles
- Add tool results back to conversation context
- Handle large tool results with intelligent truncation
- Comprehensive tool call history tracking

4.2. **State Management**
- Track tool execution state across sessions
- Enhanced session-based tool history
- Proper cleanup on session reset
- Session persistence for HTTP transport with resumability
- Authorization state management

## Technical Implementation

### Configuration

- Extend `Config` struct to include MCP server definitions
- Handle globally configured MCP servers
- Add validation for MCP server configurations including transport types
- Basic environment variable substitution
- Support for OAuth 2.1 authorization configuration
- Protocol version negotiation

### MCP Protocol
- Use existing Rust MCP libraries, or implement JSON-RPC 2.0 client
- Handle protocol negotiation and capability exchange
- Support multiple MCP protocol versions
- Implement proper initialization handshake with enhanced server capabilities
- Support for completions capability for argument autocompletion

### Transport Support
- **STDIO**: Primary transport for local tool servers
- Process management with tokio-process
- JSON-RPC over stdin/stdout with batch support
- Process lifecycle management
- **Streamable HTTP**: Updated transport replacing HTTP+SSE
- Improved flexibility and reliability
- HTTP POST for client-to-server communication
- Enhanced streaming capabilities
- Session management with proper resumability
- Connection health monitoring

### Authorization (OAuth 2.1)
- **Mandatory PKCE**: All OAuth implementations must use PKCE
- **Dynamic Client Registration**: Support where available
- **Authorization Code Flow**: Standard OAuth flow for user consent
- **Token Management**: Secure storage, refresh, and revocation
- **Server Discovery**: Support for `.well-known/oauth-authorization-server`
- **Scope Management**: Handle different permission scopes per server

### Security Considerations
- User consent flows for destructive operations
- Enhanced input validation for tool parameters with annotation awareness
- OAuth 2.1 security best practices
- Tool safety based on behavioral annotations
- For HTTP transport:
- Validate `Origin` headers to prevent DNS rebinding attacks
- Use HTTPS for production deployments
- Proper session validation and CSRF protection
- Secure authorization token handling

### Performance
- Async/await for non-blocking operations
- JSON-RPC batch processing for efficiency
- Connection reuse and intelligent pooling
- Enhanced caching for tool definitions and authorization tokens
- Memory management for tool results with streaming support
- Progress tracking for long-running operations

## Tenx Integration Points

### Context System Integration
- **MCPContext provider**: New context type implementing `ContextProvider` trait
- **ContextManager integration**: Add MCP tools as `Context::Mcp(MCPContext)` variant
- **Dynamic context updates**: Refresh MCP tool definitions when server capabilities change
- **Multi-content support**: Handle text, image, and audio content in tool results
- **Event integration**: Use existing `EventSender` for MCP operation progress

### Model System Integration
- **Chat trait extension**: Add `add_mcp_tools()` method to all Chat implementations
- **Tool call parsing**: Extend `ModelResponse` with new `tool_calls` field
- **Function calling**: Format MCP tools for OpenAI-style function calling
- **Batch operations**: Support JSON-RPC batching in model interactions
- **Error propagation**: Use existing `TenxError` enum with new `Mcp` variant

### Strategy System Integration
- **Tool-aware strategies**: Extend `ActionStrategy` to handle tool execution
- **Code strategy**: Add MCP tool calls for code analysis and modification
- **Fix strategy**: Use MCP tools for error diagnosis and resolution
- **Strategy state**: Track tool execution in `StrategyState` variants
- **User consent**: Integrate with existing user interaction patterns

### Configuration Integration
- **Config extension**: Add `mcp: MCPConfig` field to main Config struct
- **Environment loading**: Use existing `load_env()` pattern for MCP secrets
- **Path normalization**: Use existing `normalize_path()` for MCP server paths
- **Validation**: Integrate MCP config validation with existing checks

### Session Integration
- **Session persistence**: Store MCP tool history in session serialization
- **State rollback**: Handle MCP tool state in session reset operations
- **Action tracking**: Include MCP tool calls in Action step history
- **Context management**: Persist MCP server connections across sessions

### Error Handling Integration
- **TenxError extension**: Add MCP-specific error variants
- **Result propagation**: Use existing `error::Result<T>` pattern
- **Error reporting**: Integrate with existing event system for error display
- **Retry logic**: Use existing retry patterns for MCP server failures

### Event System Integration
- **Progress tracking**: Use `Event::PromptStart/End` pattern for tool execution
- **User feedback**: Add new event types for MCP operations
- **Logging integration**: Use existing log level system for MCP debugging
- **Throttling**: Integrate with existing throttle system for rate limits

## File Structure

```
crates/libtenx/src/
├── mcp/
│   ├── mod.rs                    # Public MCP API
│   ├── client/
│   │   ├── mod.rs                # Client trait and types
│   │   ├── stdio.rs              # STDIO transport
│   │   ├── streamable_http.rs    # Streamable HTTP transport
│   │   └── batch.rs              # JSON-RPC batch support
│   ├── auth/
│   │   ├── mod.rs                # Authorization framework
│   │   ├── oauth.rs              # OAuth 2.1 implementation
│   │   ├── discovery.rs          # Server discovery
│   │   └── tokens.rs             # Token management
│   ├── config.rs                 # MCP configuration
│   ├── engine.rs                 # Tool execution engine
│   ├── protocol.rs               # JSON-RPC 2.0 with batching
│   ├── tools.rs                  # Tool management with annotations
│   ├── context.rs                # MCP context provider
│   ├── progress.rs               # Progress tracking
│   └── error.rs                  # MCP error types
└── model/
    └── tools.rs                  # Tool call parsing and execution
```

## Implementation Phases

### Phase 1 (MVP): MCP Protocol Support via STDIO
- Config loader via mcp.json
- Basic STDIO client implementation
- Tool discovery and execution with annotations
- Enhanced error handling and progress tracking

### Phase 2: HTTP and Authorization
- OAuth 2.1 implementation with PKCE
- Streamable HTTP transport
- Dynamic client registration
- Secure token management

### Phase 3: Advanced Features
- JSON-RPC 2.0 with batch support
- Advanced tool annotation handling
- Multi-format content type support (images, audio)
- Performance optimizations: caching, pooling, concurrent
- Enhanced user consent flows

### Phase 4: Polish and Production
- Advanced error recovery
- Monitoring and metrics
- Production security hardening

## Testing Strategy

- Unit tests for STDIO and Streamable HTTP transports
- Integration tests with mock MCP servers supporting multiple versions
- OAuth 2.1 flow testing with mock authorization servers
- End-to-end tests with real MCP servers (git, filesystem)
- Tool annotation parsing and validation tests
- Authorization flow testing
- Protocol compliance tests


## Success Criteria

1. Successfully connect to and use MCP servers supporting distinct protocol versions
2. Support both STDIO and Streamable HTTP transports reliably
3. Execute tool calls with proper authorization and annotation handling
4. Handle OAuth 2.1 flows seamlessly for remote servers
5. Provide enhanced error messages and progress tracking
6. Integrate batch processing, and concurrent requests for improved performance
7. Ensure user consent flows work properly for destructive operations

## Extra notes and questions
- Should we need to support SSE transport? it is being used by servers, but [deprecated as of 2025-11-05](https://modelcontextprotocol.io/docs/concepts/transports#server-sent-events-sse-deprecated)
