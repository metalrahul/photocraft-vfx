// A small C API over OpenColorIO for photocraft-ocio (src/lib.rs). Only what PhotoCraft
// needs: load a config, list its colour spaces, and convert float RGBA between two of them on
// the CPU. Every OCIO exception is caught here and returned as text, never thrown across the
// FFI boundary.

#include <OpenColorIO/OpenColorIO.h>

#include <cstring>
#include <exception>
#include <string>
#include <vector>

namespace OCIO = OCIO_NAMESPACE;

struct pc_ocio_config {
    OCIO::ConstConfigRcPtr config;
    std::string name;
    // Active colour spaces, in config order, with their families (owned here so the C strings
    // handed out stay valid as long as the config).
    std::vector<std::string> names;
    std::vector<std::string> families;
};

struct pc_ocio_proc {
    OCIO::ConstCPUProcessorRcPtr cpu;
    bool noop;
};

static void set_err(char* err, size_t len, const char* msg) {
    if (err && len) {
        std::strncpy(err, msg, len - 1);
        err[len - 1] = '\0';
    }
}

static pc_ocio_config* wrap(OCIO::ConstConfigRcPtr cfg) {
    auto* out = new pc_ocio_config();
    out->config = cfg;
    const char* name = cfg->getName();
    out->name = name ? name : "";
    const int n = cfg->getNumColorSpaces(OCIO::SEARCH_REFERENCE_SPACE_ALL, OCIO::COLORSPACE_ACTIVE);
    for (int i = 0; i < n; ++i) {
        const char* cs = cfg->getColorSpaceNameByIndex(OCIO::SEARCH_REFERENCE_SPACE_ALL, OCIO::COLORSPACE_ACTIVE, i);
        if (!cs) continue;
        auto space = cfg->getColorSpace(cs);
        const char* family = space ? space->getFamily() : nullptr;
        out->names.emplace_back(cs);
        out->families.emplace_back(family ? family : "");
    }
    return out;
}

extern "C" {

const char* pc_ocio_version() { return OCIO::GetVersion(); }

// A config from a file path or an `ocio://` built-in URI.
pc_ocio_config* pc_ocio_config_from_uri(const char* uri, char* err, size_t err_len) {
    try {
        return wrap(OCIO::Config::CreateFromFile(uri));
    } catch (const std::exception& e) {
        set_err(err, err_len, e.what());
    } catch (...) {
        set_err(err, err_len, "unknown OpenColorIO error");
    }
    return nullptr;
}

// The config `$OCIO` names (OCIO's own rules, as in Nuke).
pc_ocio_config* pc_ocio_config_from_env(char* err, size_t err_len) {
    try {
        return wrap(OCIO::Config::CreateFromEnv());
    } catch (const std::exception& e) {
        set_err(err, err_len, e.what());
    } catch (...) {
        set_err(err, err_len, "unknown OpenColorIO error");
    }
    return nullptr;
}

void pc_ocio_config_free(pc_ocio_config* c) { delete c; }

const char* pc_ocio_config_name(const pc_ocio_config* c) { return c->name.c_str(); }

int pc_ocio_colorspace_count(const pc_ocio_config* c) { return static_cast<int>(c->names.size()); }

const char* pc_ocio_colorspace_name(const pc_ocio_config* c, int i) {
    return (i >= 0 && i < static_cast<int>(c->names.size())) ? c->names[i].c_str() : nullptr;
}

const char* pc_ocio_colorspace_family(const pc_ocio_config* c, int i) {
    return (i >= 0 && i < static_cast<int>(c->families.size())) ? c->families[i].c_str() : nullptr;
}

// The colour space a role (scene_linear, compositing_log, …) names, or null.
const char* pc_ocio_role(const pc_ocio_config* c, const char* role) {
    try {
        if (!c->config->hasRole(role)) return nullptr;
        auto cs = c->config->getColorSpace(role);
        return cs ? cs->getName() : nullptr;
    } catch (...) {
        return nullptr;
    }
}

// A CPU processor from colour space `src` to `dst` (names, aliases or roles), float in and out.
pc_ocio_proc* pc_ocio_processor(const pc_ocio_config* c, const char* src, const char* dst, char* err, size_t err_len) {
    try {
        auto proc = c->config->getProcessor(src, dst);
        auto* out = new pc_ocio_proc();
        out->cpu = proc->getDefaultCPUProcessor();
        out->noop = proc->isNoOp();
        return out;
    } catch (const std::exception& e) {
        set_err(err, err_len, e.what());
    } catch (...) {
        set_err(err, err_len, "unknown OpenColorIO error");
    }
    return nullptr;
}

int pc_ocio_proc_is_noop(const pc_ocio_proc* p) { return p->noop ? 1 : 0; }

// Converts `width × height` packed float RGBA pixels in place (alpha passes through).
void pc_ocio_proc_apply_rgba(const pc_ocio_proc* p, float* data, long width, long height) {
    if (p->noop || width <= 0 || height <= 0) return;
    try {
        OCIO::PackedImageDesc img(data, width, height, 4);
        p->cpu->apply(img);
    } catch (...) {
        // Left unchanged: the caller validated the processor when it was made.
    }
}

void pc_ocio_proc_free(pc_ocio_proc* p) { delete p; }

}  // extern "C"
