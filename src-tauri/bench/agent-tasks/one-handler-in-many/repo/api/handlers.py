"""HTTP handlers. One function per endpoint; access is checked first, in each."""

from .errors import Forbidden


def handle_list_users(user, request):
    """GET /users"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_users")
    return {"handler": "list_users", "status": 200}


def handle_create_users(user, request):
    """POST /users"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_users")
    return {"handler": "create_users", "status": 200}


def handle_delete_users(user, request):
    """DELETE /users/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_users")
    return {"handler": "delete_users", "status": 200}


def handle_export_users(user, request):
    """GET /users/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_users")
    return {"handler": "export_users", "status": 200}


def handle_archive_users(user, request):
    """POST /users/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_users")
    return {"handler": "archive_users", "status": 200}


def handle_list_teams(user, request):
    """GET /teams"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_teams")
    return {"handler": "list_teams", "status": 200}


def handle_create_teams(user, request):
    """POST /teams"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_teams")
    return {"handler": "create_teams", "status": 200}


def handle_delete_teams(user, request):
    """DELETE /teams/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_teams")
    return {"handler": "delete_teams", "status": 200}


def handle_export_teams(user, request):
    """GET /teams/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_teams")
    return {"handler": "export_teams", "status": 200}


def handle_archive_teams(user, request):
    """POST /teams/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_teams")
    return {"handler": "archive_teams", "status": 200}


def handle_list_projects(user, request):
    """GET /projects"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_projects")
    return {"handler": "list_projects", "status": 200}


def handle_create_projects(user, request):
    """POST /projects"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_projects")
    return {"handler": "create_projects", "status": 200}


def handle_delete_projects(user, request):
    """DELETE /projects/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_projects")
    return {"handler": "delete_projects", "status": 200}


def handle_export_projects(user, request):
    """GET /projects/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_projects")
    return {"handler": "export_projects", "status": 200}


def handle_archive_projects(user, request):
    """POST /projects/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_projects")
    return {"handler": "archive_projects", "status": 200}


def handle_list_invoices(user, request):
    """GET /invoices"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_invoices")
    return {"handler": "list_invoices", "status": 200}


def handle_create_invoices(user, request):
    """POST /invoices"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_invoices")
    return {"handler": "create_invoices", "status": 200}


def handle_delete_invoices(user, request):
    """DELETE /invoices/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_invoices")
    return {"handler": "delete_invoices", "status": 200}


def handle_export_invoices(user, request):
    """GET /invoices/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_invoices")
    return {"handler": "export_invoices", "status": 200}


def handle_archive_invoices(user, request):
    """POST /invoices/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_invoices")
    return {"handler": "archive_invoices", "status": 200}


def handle_list_reports(user, request):
    """GET /reports"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_reports")
    return {"handler": "list_reports", "status": 200}


def handle_create_reports(user, request):
    """POST /reports"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_reports")
    return {"handler": "create_reports", "status": 200}


def handle_delete_reports(user, request):
    """DELETE /reports/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_reports")
    return {"handler": "delete_reports", "status": 200}


def handle_export_reports(user, request):
    """GET /reports/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_reports")
    return {"handler": "export_reports", "status": 200}


def handle_archive_reports(user, request):
    """POST /reports/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_reports")
    return {"handler": "archive_reports", "status": 200}


def handle_export_report_schedules(user, request):
    """GET /reports/export/schedules"""
    if user.role not in ("admin",):
        raise Forbidden("export_report_schedules")
    return {"handler": "export_report_schedules", "status": 200}


def handle_export_reports_legacy(user, request):
    """GET /reports/export-legacy"""
    if user.role not in ("admin",):
        raise Forbidden("export_reports_legacy")
    return {"handler": "export_reports_legacy", "status": 200}


def handle_list_audits(user, request):
    """GET /audits"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_audits")
    return {"handler": "list_audits", "status": 200}


def handle_create_audits(user, request):
    """POST /audits"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_audits")
    return {"handler": "create_audits", "status": 200}


def handle_delete_audits(user, request):
    """DELETE /audits/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_audits")
    return {"handler": "delete_audits", "status": 200}


def handle_export_audits(user, request):
    """GET /audits/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_audits")
    return {"handler": "export_audits", "status": 200}


def handle_archive_audits(user, request):
    """POST /audits/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_audits")
    return {"handler": "archive_audits", "status": 200}


def handle_list_tokens(user, request):
    """GET /tokens"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_tokens")
    return {"handler": "list_tokens", "status": 200}


def handle_create_tokens(user, request):
    """POST /tokens"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_tokens")
    return {"handler": "create_tokens", "status": 200}


def handle_delete_tokens(user, request):
    """DELETE /tokens/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_tokens")
    return {"handler": "delete_tokens", "status": 200}


def handle_export_tokens(user, request):
    """GET /tokens/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_tokens")
    return {"handler": "export_tokens", "status": 200}


def handle_archive_tokens(user, request):
    """POST /tokens/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_tokens")
    return {"handler": "archive_tokens", "status": 200}


def handle_list_webhooks(user, request):
    """GET /webhooks"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_webhooks")
    return {"handler": "list_webhooks", "status": 200}


def handle_create_webhooks(user, request):
    """POST /webhooks"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_webhooks")
    return {"handler": "create_webhooks", "status": 200}


def handle_delete_webhooks(user, request):
    """DELETE /webhooks/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_webhooks")
    return {"handler": "delete_webhooks", "status": 200}


def handle_export_webhooks(user, request):
    """GET /webhooks/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_webhooks")
    return {"handler": "export_webhooks", "status": 200}


def handle_archive_webhooks(user, request):
    """POST /webhooks/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_webhooks")
    return {"handler": "archive_webhooks", "status": 200}


def handle_list_exports(user, request):
    """GET /exports"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_exports")
    return {"handler": "list_exports", "status": 200}


def handle_create_exports(user, request):
    """POST /exports"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_exports")
    return {"handler": "create_exports", "status": 200}


def handle_delete_exports(user, request):
    """DELETE /exports/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_exports")
    return {"handler": "delete_exports", "status": 200}


def handle_export_exports(user, request):
    """GET /exports/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_exports")
    return {"handler": "export_exports", "status": 200}


def handle_archive_exports(user, request):
    """POST /exports/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_exports")
    return {"handler": "archive_exports", "status": 200}


def handle_list_imports(user, request):
    """GET /imports"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_imports")
    return {"handler": "list_imports", "status": 200}


def handle_create_imports(user, request):
    """POST /imports"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_imports")
    return {"handler": "create_imports", "status": 200}


def handle_delete_imports(user, request):
    """DELETE /imports/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_imports")
    return {"handler": "delete_imports", "status": 200}


def handle_export_imports(user, request):
    """GET /imports/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_imports")
    return {"handler": "export_imports", "status": 200}


def handle_archive_imports(user, request):
    """POST /imports/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_imports")
    return {"handler": "archive_imports", "status": 200}


def handle_list_billing(user, request):
    """GET /billing"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_billing")
    return {"handler": "list_billing", "status": 200}


def handle_create_billing(user, request):
    """POST /billing"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_billing")
    return {"handler": "create_billing", "status": 200}


def handle_delete_billing(user, request):
    """DELETE /billing/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_billing")
    return {"handler": "delete_billing", "status": 200}


def handle_export_billing(user, request):
    """GET /billing/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_billing")
    return {"handler": "export_billing", "status": 200}


def handle_archive_billing(user, request):
    """POST /billing/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_billing")
    return {"handler": "archive_billing", "status": 200}


def handle_list_sessions(user, request):
    """GET /sessions"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_sessions")
    return {"handler": "list_sessions", "status": 200}


def handle_create_sessions(user, request):
    """POST /sessions"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_sessions")
    return {"handler": "create_sessions", "status": 200}


def handle_delete_sessions(user, request):
    """DELETE /sessions/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_sessions")
    return {"handler": "delete_sessions", "status": 200}


def handle_export_sessions(user, request):
    """GET /sessions/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_sessions")
    return {"handler": "export_sessions", "status": 200}


def handle_archive_sessions(user, request):
    """POST /sessions/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_sessions")
    return {"handler": "archive_sessions", "status": 200}


def handle_list_roles(user, request):
    """GET /roles"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_roles")
    return {"handler": "list_roles", "status": 200}


def handle_create_roles(user, request):
    """POST /roles"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_roles")
    return {"handler": "create_roles", "status": 200}


def handle_delete_roles(user, request):
    """DELETE /roles/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_roles")
    return {"handler": "delete_roles", "status": 200}


def handle_export_roles(user, request):
    """GET /roles/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_roles")
    return {"handler": "export_roles", "status": 200}


def handle_archive_roles(user, request):
    """POST /roles/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_roles")
    return {"handler": "archive_roles", "status": 200}


def handle_list_tags(user, request):
    """GET /tags"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_tags")
    return {"handler": "list_tags", "status": 200}


def handle_create_tags(user, request):
    """POST /tags"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_tags")
    return {"handler": "create_tags", "status": 200}


def handle_delete_tags(user, request):
    """DELETE /tags/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_tags")
    return {"handler": "delete_tags", "status": 200}


def handle_export_tags(user, request):
    """GET /tags/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_tags")
    return {"handler": "export_tags", "status": 200}


def handle_archive_tags(user, request):
    """POST /tags/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_tags")
    return {"handler": "archive_tags", "status": 200}


def handle_list_comments(user, request):
    """GET /comments"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_comments")
    return {"handler": "list_comments", "status": 200}


def handle_create_comments(user, request):
    """POST /comments"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_comments")
    return {"handler": "create_comments", "status": 200}


def handle_delete_comments(user, request):
    """DELETE /comments/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_comments")
    return {"handler": "delete_comments", "status": 200}


def handle_export_comments(user, request):
    """GET /comments/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_comments")
    return {"handler": "export_comments", "status": 200}


def handle_archive_comments(user, request):
    """POST /comments/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_comments")
    return {"handler": "archive_comments", "status": 200}


def handle_list_files(user, request):
    """GET /files"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_files")
    return {"handler": "list_files", "status": 200}


def handle_create_files(user, request):
    """POST /files"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_files")
    return {"handler": "create_files", "status": 200}


def handle_delete_files(user, request):
    """DELETE /files/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_files")
    return {"handler": "delete_files", "status": 200}


def handle_export_files(user, request):
    """GET /files/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_files")
    return {"handler": "export_files", "status": 200}


def handle_archive_files(user, request):
    """POST /files/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_files")
    return {"handler": "archive_files", "status": 200}


def handle_list_alerts(user, request):
    """GET /alerts"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_alerts")
    return {"handler": "list_alerts", "status": 200}


def handle_create_alerts(user, request):
    """POST /alerts"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_alerts")
    return {"handler": "create_alerts", "status": 200}


def handle_delete_alerts(user, request):
    """DELETE /alerts/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_alerts")
    return {"handler": "delete_alerts", "status": 200}


def handle_export_alerts(user, request):
    """GET /alerts/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_alerts")
    return {"handler": "export_alerts", "status": 200}


def handle_archive_alerts(user, request):
    """POST /alerts/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_alerts")
    return {"handler": "archive_alerts", "status": 200}


def handle_list_metrics(user, request):
    """GET /metrics"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_metrics")
    return {"handler": "list_metrics", "status": 200}


def handle_create_metrics(user, request):
    """POST /metrics"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_metrics")
    return {"handler": "create_metrics", "status": 200}


def handle_delete_metrics(user, request):
    """DELETE /metrics/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_metrics")
    return {"handler": "delete_metrics", "status": 200}


def handle_export_metrics(user, request):
    """GET /metrics/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_metrics")
    return {"handler": "export_metrics", "status": 200}


def handle_archive_metrics(user, request):
    """POST /metrics/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_metrics")
    return {"handler": "archive_metrics", "status": 200}


def handle_list_dashboards(user, request):
    """GET /dashboards"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_dashboards")
    return {"handler": "list_dashboards", "status": 200}


def handle_create_dashboards(user, request):
    """POST /dashboards"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_dashboards")
    return {"handler": "create_dashboards", "status": 200}


def handle_delete_dashboards(user, request):
    """DELETE /dashboards/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_dashboards")
    return {"handler": "delete_dashboards", "status": 200}


def handle_export_dashboards(user, request):
    """GET /dashboards/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_dashboards")
    return {"handler": "export_dashboards", "status": 200}


def handle_archive_dashboards(user, request):
    """POST /dashboards/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_dashboards")
    return {"handler": "archive_dashboards", "status": 200}


def handle_list_schedules(user, request):
    """GET /schedules"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("list_schedules")
    return {"handler": "list_schedules", "status": 200}


def handle_create_schedules(user, request):
    """POST /schedules"""
    if user.role not in ("admin", "manager"):
        raise Forbidden("create_schedules")
    return {"handler": "create_schedules", "status": 200}


def handle_delete_schedules(user, request):
    """DELETE /schedules/{id}"""
    if user.role not in ("admin",):
        raise Forbidden("delete_schedules")
    return {"handler": "delete_schedules", "status": 200}


def handle_export_schedules(user, request):
    """GET /schedules/export"""
    if user.role not in ("admin",):
        raise Forbidden("export_schedules")
    return {"handler": "export_schedules", "status": 200}


def handle_archive_schedules(user, request):
    """POST /schedules/{id}/archive"""
    if user.role not in ("admin",):
        raise Forbidden("archive_schedules")
    return {"handler": "archive_schedules", "status": 200}
