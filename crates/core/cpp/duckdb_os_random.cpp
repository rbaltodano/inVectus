// inVectus: give DuckDB's built-in AES-GCM module the operating system's random generator.
//
// The bundled DuckDB (v2.0) encrypts databases with mbedtls AES-256-GCM but draws nonces and keys
// from `RandomEngine`, a non-cryptographic generator, so it refuses to write encrypted files unless
// `force_mbedtls_unsafe` is set. DuckDB lets a database use any `EncryptionUtil` set in its config
// (that is how httpfs plugs in OpenSSL). This file installs one that keeps DuckDB's mbedtls cipher
// unchanged and overrides only `GenerateRandomData` with the OS CSPRNG: BCryptGenRandom on Windows,
// getentropy(2) on macOS and Linux. Every nonce, temp-file key and parquet key DuckDB generates
// goes through it. See docs/encryption-plan.md (phase 2).

#include "duckdb/common/exception.hpp"
#include "duckdb/main/capi/capi_internal.hpp"
#include "duckdb/main/config.hpp"
#include "duckdb/main/database.hpp"
#include "mbedtls_wrapper.hpp"

#include <atomic>
#include <cstddef>
#include <cstdint>

#if defined(_WIN32)
#include <windows.h>
#include <bcrypt.h>
#else
#include <unistd.h>
#if defined(__APPLE__)
#include <sys/random.h>
#endif
#endif

namespace {

std::atomic<uint64_t> os_random_calls {0};

bool OsRandom(uint8_t *data, size_t len) {
#if defined(_WIN32)
	while (len > 0) {
		const ULONG chunk = len > 0x10000000 ? 0x10000000 : static_cast<ULONG>(len);
		if (!BCRYPT_SUCCESS(BCryptGenRandom(nullptr, data, chunk, BCRYPT_USE_SYSTEM_PREFERRED_RNG))) {
			return false;
		}
		data += chunk;
		len -= chunk;
	}
	return true;
#else
	// getentropy fills at most 256 bytes per call and blocks until the kernel pool is seeded.
	while (len > 0) {
		const size_t chunk = len > 256 ? 256 : len;
		if (getentropy(data, chunk) != 0) {
			return false;
		}
		data += chunk;
		len -= chunk;
	}
	return true;
#endif
}

using duckdb_mbedtls::MbedTlsWrapper;

class OsRandomAesState final : public MbedTlsWrapper::AESStateMBEDTLS {
public:
	using MbedTlsWrapper::AESStateMBEDTLS::AESStateMBEDTLS;

	void GenerateRandomData(duckdb::data_ptr_t data, duckdb::idx_t len) override {
		if (!OsRandom(data, len)) {
			throw duckdb::InternalException("the operating system's random generator failed");
		}
		os_random_calls.fetch_add(1, std::memory_order_relaxed);
	}
};

class OsRandomAesFactory final : public MbedTlsWrapper::AESStateMBEDTLSFactory {
public:
	duckdb::shared_ptr<duckdb::EncryptionState>
	CreateEncryptionState(duckdb::unique_ptr<duckdb::EncryptionStateMetadata> metadata) const override {
		return duckdb::make_shared_ptr<OsRandomAesState>(std::move(metadata));
	}
};

} // namespace

// Install the OS-random encryption module on a database opened through the C API. Call it
// right after duckdb_open_ext, before anything attaches or writes. Returns 0 on success.
extern "C" int invectus_duckdb_use_os_random(duckdb_database database) {
	try {
		auto *wrapper = reinterpret_cast<duckdb::DatabaseWrapper *>(database);
		if (!wrapper || !wrapper->database) {
			return 1;
		}
		auto &config = duckdb::DBConfig::GetConfig(*wrapper->database->instance);
		config.encryption_util = duckdb::make_shared_ptr<OsRandomAesFactory>();
		return 0;
	} catch (...) {
		return 2;
	}
}

// How many times DuckDB asked the OS generator for random bytes (tests check it is used).
extern "C" uint64_t invectus_duckdb_os_random_calls() {
	return os_random_calls.load(std::memory_order_relaxed);
}
