/*
 * nottsd - owns org.freedesktop.Notifications on the session bus and hands every notification to finedesktop's
 * notification center (`finedesktop ctl notify send`), so notify-send, Discord, Telegram, Element, Firefox and the
 * rest end up in one history, grouped by the app that sent them. No window, no sound, no state of its own.
 *
 *   build:  make            (libsystemd, or elogind's sd-bus on Void)
 *   run:    nottsd &        (from the session script; a second copy finds the name taken and exits)
 *   FD_NOTTS_CTL=cmd        command used instead of `finedesktop` (the tests point it at a logger)
 */
#include <ctype.h>
#include <errno.h>
#include <signal.h>
#include <spawn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#if __has_include(<systemd/sd-bus.h>)
#include <systemd/sd-bus.h>
#else
#include <elogind/sd-bus.h>
#endif

extern char **environ;

#define TEXT_MAX 2048
static uint32_t next_id = 1;

/* "<b>hi</b>\n&amp;" -> "hi &": ctl takes one line, and the bar draws plain text */
static void plain(const char *in, char *out, size_t cap)
{
	size_t n = 0;
	int tag = 0;

	for (; *in && n + 1 < cap; in++) {
		if (*in == '<' && (isalpha((unsigned char)in[1]) || in[1] == '/')) {
			tag = 1;
		} else if (*in == '>' && tag) {
			tag = 0;
		} else if (!tag) {
			if (!strncmp(in, "&amp;", 5)) { out[n++] = '&'; in += 4; }
			else if (!strncmp(in, "&lt;", 4)) { out[n++] = '<'; in += 3; }
			else if (!strncmp(in, "&gt;", 4)) { out[n++] = '>'; in += 3; }
			else if (!strncmp(in, "&quot;", 6)) { out[n++] = '"'; in += 5; }
			else out[n++] = (*in == '\n' || *in == '\r' || *in == '\t') ? ' ' : *in;
		}
	}
	out[n] = '\0';
}

/* one word, no whitespace: the app name and the tag are single ctl arguments */
static void word(const char *in, char *out, size_t cap, const char *dflt)
{
	size_t n = 0;

	for (; *in && n + 1 < cap; in++)
		out[n++] = isspace((unsigned char)*in) ? '_' : *in;
	out[n] = '\0';
	if (!n || out[0] == '-')
		snprintf(out, cap, "%s", dflt);
}

static void post(const char *app, const char *urg, const char *tag, const char *text)
{
	const char *ctl = getenv("FD_NOTTS_CTL");
	char *argv[16];
	int i = 0;
	pid_t pid;

	argv[i++] = (char *)(ctl && *ctl ? ctl : "finedesktop");
	argv[i++] = "ctl";
	argv[i++] = "notify";
	argv[i++] = "send";
	argv[i++] = "-a";
	argv[i++] = (char *)app;
	argv[i++] = "-u";
	argv[i++] = (char *)urg;
	if (*tag) {
		argv[i++] = "-t";
		argv[i++] = (char *)tag;
	}
	argv[i++] = "--";
	argv[i++] = (char *)text;
	argv[i] = NULL;
	if (posix_spawnp(&pid, argv[0], NULL, NULL, argv, environ) != 0)
		fprintf(stderr, "nottsd: cannot run %s\n", argv[0]);
}

static int m_caps(sd_bus_message *m, void *u, sd_bus_error *err)
{
	sd_bus_message *r = NULL;
	int rc;

	(void)u; (void)err;
	if ((rc = sd_bus_message_new_method_return(m, &r)) < 0)
		return rc;
	if ((rc = sd_bus_message_append(r, "as", 2, "body", "persistence")) < 0)
		return rc;
	rc = sd_bus_send(NULL, r, NULL);
	sd_bus_message_unref(r);
	return rc;
}

static int m_info(sd_bus_message *m, void *u, sd_bus_error *err)
{
	(void)u; (void)err;
	return sd_bus_reply_method_return(m, "ssss", "finedesktop-notts", "finedesktop", "1", "1.2");
}

static int m_close(sd_bus_message *m, void *u, sd_bus_error *err)
{
	uint32_t id;
	int r = sd_bus_message_read(m, "u", &id);

	(void)u; (void)err;
	if (r < 0)
		return r;
	return sd_bus_reply_method_return(m, "");
}

static int m_notify(sd_bus_message *m, void *u, sd_bus_error *err)
{
	const char *app, *icon, *summary, *body;
	char urg[9] = "normal", tag[64] = "", a[128], text[TEXT_MAX + 4], s[TEXT_MAX / 2], b[TEXT_MAX / 2];
	uint32_t rid;
	int32_t timeout;
	int r;

	(void)u; (void)err;
	if ((r = sd_bus_message_read(m, "susss", &app, &rid, &icon, &summary, &body)) < 0)
		return r;
	if ((r = sd_bus_message_skip(m, "as")) < 0)
		return r;
	if ((r = sd_bus_message_enter_container(m, 'a', "{sv}")) < 0)
		return r;
	while ((r = sd_bus_message_enter_container(m, 'e', "sv")) > 0) {
		const char *key, *cont = NULL;
		char t;

		if ((r = sd_bus_message_read(m, "s", &key)) < 0)
			return r;
		r = sd_bus_message_peek_type(m, &t, &cont);
		if (r >= 0 && !strcmp(key, "urgency") && cont && !strcmp(cont, "y")) {
			uint8_t v;

			if ((r = sd_bus_message_read(m, "v", "y", &v)) < 0)
				return r;
			snprintf(urg, sizeof(urg), "%s", v == 0 ? "low" : v >= 2 ? "critical" : "normal");
		} else if (r >= 0 && !strcmp(key, "category") && cont && !strcmp(cont, "s")) {
			const char *c;

			if ((r = sd_bus_message_read(m, "v", "s", &c)) < 0)
				return r;
			word(c, tag, sizeof(tag), "");
		} else if ((r = sd_bus_message_skip(m, "v")) < 0) {
			return r;
		}
		if ((r = sd_bus_message_exit_container(m)) < 0)
			return r;
	}
	if (r < 0 || (r = sd_bus_message_exit_container(m)) < 0)
		return r;
	if ((r = sd_bus_message_read(m, "i", &timeout)) < 0)
		return r;

	word(app, a, sizeof(a), "unknown");
	plain(summary, s, sizeof(s));
	plain(body, b, sizeof(b));
	snprintf(text, sizeof(text), "%s%s%s", s, *s && *b ? ": " : "", b);
	if (!*text)
		snprintf(text, sizeof(text), "(empty)");
	post(a, urg, tag, text);
	return sd_bus_reply_method_return(m, "u", rid ? rid : next_id++);
}

static const sd_bus_vtable vt[] = {
	SD_BUS_VTABLE_START(0),
	SD_BUS_METHOD("GetCapabilities", "", "as", m_caps, SD_BUS_VTABLE_UNPRIVILEGED),
	SD_BUS_METHOD("Notify", "susssasa{sv}i", "u", m_notify, SD_BUS_VTABLE_UNPRIVILEGED),
	SD_BUS_METHOD("CloseNotification", "u", "", m_close, SD_BUS_VTABLE_UNPRIVILEGED),
	SD_BUS_METHOD("GetServerInformation", "", "ssss", m_info, SD_BUS_VTABLE_UNPRIVILEGED),
	SD_BUS_SIGNAL("NotificationClosed", "uu", 0),
	SD_BUS_SIGNAL("ActionInvoked", "us", 0),
	SD_BUS_VTABLE_END
};

int main(void)
{
	sd_bus *bus = NULL;
	struct sigaction sa = { .sa_handler = SIG_IGN, .sa_flags = SA_NOCLDWAIT };
	int r;

	sigaction(SIGCHLD, &sa, NULL); /* the ctl children are never waited for */
	if ((r = sd_bus_open_user(&bus)) < 0) {
		fprintf(stderr, "nottsd: no session bus: %s\n", strerror(-r));
		return 1;
	}
	if ((r = sd_bus_add_object_vtable(bus, NULL, "/org/freedesktop/Notifications", "org.freedesktop.Notifications", vt, NULL)) < 0) {
		fprintf(stderr, "nottsd: %s\n", strerror(-r));
		return 1;
	}
	if ((r = sd_bus_request_name(bus, "org.freedesktop.Notifications", 0)) < 0) {
		fprintf(stderr, "nottsd: the name is taken (another notification daemon runs): %s\n", strerror(-r));
		return r == -EEXIST ? 0 : 1;
	}
	for (;;) {
		r = sd_bus_process(bus, NULL);
		if (r < 0) {
			fprintf(stderr, "nottsd: %s\n", strerror(-r));
			return 1;
		}
		if (r > 0)
			continue;
		if ((r = sd_bus_wait(bus, UINT64_MAX)) < 0 && r != -EINTR) {
			fprintf(stderr, "nottsd: %s\n", strerror(-r));
			return 1;
		}
	}
}
