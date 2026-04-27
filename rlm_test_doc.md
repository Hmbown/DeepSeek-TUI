This is a test document for rlm_process.

# Security Model Overview

The system uses a zero-trust architecture where every request is authenticated and authorized independently.

## Authentication

Authentication is handled via JWT tokens with a 15-minute expiry. Refresh tokens are stored in an HTTP-only cookie.

## Authorization

Role-based access control (RBAC) is used with three tiers:
- Admin: full access
- Editor: can modify content but not system settings
- Viewer: read-only access

## Encryption

All data at rest is encrypted using AES-256-GCM. Data in transit uses TLS 1.3.

## Audit Logging

Every action is logged to an append-only audit trail stored in a separate database.

# API Endpoints

- GET /api/users - List users (Admin only)
- POST /api/users - Create user (Admin only)
- GET /api/documents - List documents
- POST /api/documents - Create document (Admin, Editor)
- PUT /api/documents/:id - Update document (Admin, Editor)
- DELETE /api/documents/:id - Delete document (Admin only)

# Data Model

User {
  id: UUID
  email: String
  role: Enum(Admin, Editor, Viewer)
  created_at: Timestamp
}

Document {
  id: UUID
  title: String
  content: Text
  author_id: UUID (FK -> User)
  created_at: Timestamp
  updated_at: Timestamp
}
