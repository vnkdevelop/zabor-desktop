using System.Diagnostics;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Zabor.Core.Security;
using Zabor.Server.Services;

try
{
    var line = Console.ReadLine() ?? throw new InvalidOperationException();
    if (line.Length > 65536) throw new InvalidOperationException();
    var options = new JsonSerializerOptions { PropertyNameCaseInsensitive = true };
    var input = JsonSerializer.Deserialize<BenchInput>(line, options) ?? throw new InvalidOperationException();
    if (input.Iterations is < 1 or > 1000 || input.BatchSize is < 1 or > 100000 ||
        input.Residents is < 1 or > 10000 || input.Warmups is < 0 or > 20 || input.Password.Length is < 1 or > 1024)
        throw new InvalidOperationException();
    var salt = Convert.FromBase64String(input.Salt);
    if (salt.Length != 16) throw new InvalidOperationException();
    var current = Rfc2898DeriveBytes.Pbkdf2(input.Password, salt, 600000, HashAlgorithmName.SHA256, 32);
    var legacy = Rfc2898DeriveBytes.Pbkdf2(input.Password, salt, 100000, HashAlgorithmName.SHA256, 32);
    var sha = SHA256.HashData(Encoding.UTF8.GetBytes(input.Password));
    var currentHash = Convert.ToBase64String(salt.Concat(current).ToArray());
    var legacyHash = Convert.ToBase64String(salt.Concat(legacy).ToArray());
    var shaHash = Convert.ToBase64String(sha);
    var rejectedPassword = "!" + input.Password;
    var proof = Convert.ToHexString(SHA256.HashData(current.Concat(legacy).Concat(sha).ToArray())).ToLowerInvariant();
    var users = Enumerable.Range(0, input.Residents * 2).Select(index => $"u{index:D6}").ToArray();
    var actual = new CallSessionRegistry();
    var single = new SingleOwnerRegistry();
    var useSingle = input.Engine == "dotnet-single-owner";
    if (!useSingle && input.Engine != "dotnet-legacy") throw new InvalidOperationException();
    for (var pair = 0; pair < input.Residents; pair++)
    {
        if (useSingle) single.Start(users[pair * 2], users[pair * 2 + 1]);
        else actual.Start(users[pair * 2], users[pair * 2 + 1]);
    }
    long successes = 0;
    long legacyResults = 0;
    void Operation(int index)
    {
        if (input.Operation.StartsWith("auth-", StringComparison.Ordinal))
        {
            var hash = input.Operation switch
            {
                "auth-current" or "auth-rejected" => currentHash,
                "auth-legacy-pbkdf2" => legacyHash,
                "auth-legacy-sha256" => shaHash,
                _ => throw new InvalidOperationException()
            };
            var candidate = input.Operation == "auth-rejected" ? rejectedPassword : input.Password;
            var accepted = EncryptionService.VerifyPassword(candidate, hash, out var isLegacy);
            if (accepted != (input.Operation != "auth-rejected")) throw new InvalidOperationException();
            if (isLegacy != input.Operation.StartsWith("auth-legacy-", StringComparison.Ordinal)) throw new InvalidOperationException();
            successes += accepted ? 1 : 0;
            legacyResults += isLegacy ? 1 : 0;
            return;
        }
        var pair = index % input.Residents;
        var first = users[pair * 2];
        var second = users[pair * 2 + 1];
        if (input.Operation == "registry-lookup")
        {
            var found = useSingle ? single.Get(first) : actual.Get(first);
            if (found is null || found.Peer(first) != second) throw new InvalidOperationException();
            successes++;
        }
        else if (input.Operation == "registry-churn")
        {
            var session = useSingle ? single.Start(first, second) : actual.Start(first, second);
            var suspended = useSingle ? single.Suspend(first) : actual.Suspend(first);
            var resumed = useSingle ? single.Resume(first, session.Id) : actual.Resume(first, session.Id);
            var stale = useSingle ? single.Resume(first, string.Empty) : actual.Resume(first, string.Empty);
            var ended = useSingle ? single.End(first) : actual.End(first);
            var removed = useSingle ? single.Get(second) : actual.Get(second);
            if (suspended is null || suspended.ExpiresAt <= 0 || resumed is null || resumed.ExpiresAt != 0 || stale is not null || ended is null || removed is not null)
                throw new InvalidOperationException();
            successes++;
        }
        else if (input.Operation == "registry-expire")
        {
            for (var seed = 0; seed < input.Residents; seed++)
            {
                if (useSingle)
                {
                    single.Start(users[seed * 2], users[seed * 2 + 1]);
                    single.Suspend(users[seed * 2]);
                }
                else
                {
                    actual.Start(users[seed * 2], users[seed * 2 + 1]);
                    actual.Suspend(users[seed * 2]);
                }
            }
            var expired = useSingle ? single.Expire(long.MaxValue) : actual.Expire(long.MaxValue);
            if (expired.Count != input.Residents) throw new InvalidOperationException();
            successes += expired.Count;
        }
        else throw new InvalidOperationException();
    }
    Console.WriteLine(JsonSerializer.Serialize(new { ready = true, fixtureProof = proof }));
    if (Console.ReadLine() != "run") throw new InvalidOperationException();
    for (var warmup = 0; warmup < input.Warmups; warmup++)
        for (var index = 0; index < input.BatchSize; index++) Operation(index);
    Thread.Sleep(1000);
    Console.WriteLine(JsonSerializer.Serialize(new { warmed = true }));
    if (Console.ReadLine() != "measure") throw new InvalidOperationException();
    successes = 0;
    legacyResults = 0;
    var samples = new double[input.Iterations];
    var collections = new[] { GC.CollectionCount(0), GC.CollectionCount(1), GC.CollectionCount(2) };
    for (var iteration = 0; iteration < input.Iterations; iteration++)
    {
        var started = Stopwatch.GetTimestamp();
        for (var index = 0; index < input.BatchSize; index++) Operation(index);
        samples[iteration] = Stopwatch.GetElapsedTime(started).TotalMilliseconds;
    }
    Console.WriteLine(JsonSerializer.Serialize(new
    {
        operation = input.Operation, engine = input.Engine, batchMs = samples,
        successes, legacyResults,
        gcCollections = Enumerable.Range(0, 3).Select(generation => GC.CollectionCount(generation) - collections[generation]).ToArray()
    }));
    if (Console.ReadLine() != "quit") throw new InvalidOperationException();
}
catch
{
    Console.Error.WriteLine("benchmark-worker-failed");
    return 1;
}
return 0;

sealed record BenchInput(string Password, string Salt, string Operation, string Engine, int Iterations, int BatchSize, int Residents, int Warmups);

sealed class SingleOwnerRegistry
{
    private readonly Dictionary<string, CallSession> _sessions = new(StringComparer.Ordinal);

    public CallSession Start(string first, string second)
    {
        End(first);
        End(second);
        var session = new CallSession(Guid.NewGuid().ToString("N"), first, second, 0);
        _sessions[first] = _sessions[second] = session;
        return session;
    }

    public CallSession? Get(string user) => _sessions.GetValueOrDefault(user);

    public CallSession? Suspend(string user)
    {
        var session = Get(user);
        if (session is null) return null;
        var next = session with { ExpiresAt = session.ExpiresAt > 0 ? session.ExpiresAt : Environment.TickCount64 + CallSessionRegistry.GraceMs };
        _sessions[next.First] = _sessions[next.Second] = next;
        return next;
    }

    public CallSession? Resume(string user, string id)
    {
        var session = Get(user);
        if (session is null || session.Id != id || session.ExpiresAt > 0 && session.ExpiresAt <= Environment.TickCount64) return null;
        var next = session with { ExpiresAt = 0 };
        _sessions[next.First] = _sessions[next.Second] = next;
        return next;
    }

    public CallSession? End(string user)
    {
        var session = Get(user);
        if (session is null) return null;
        Remove(session);
        return session;
    }

    public IReadOnlyList<CallSession> Expire(long now)
    {
        var expired = _sessions.Values.Where(session => session.ExpiresAt > 0 && session.ExpiresAt <= now).Distinct().ToArray();
        foreach (var session in expired) Remove(session);
        return expired;
    }

    private void Remove(CallSession session)
    {
        if (Get(session.First)?.Id == session.Id) _sessions.Remove(session.First);
        if (Get(session.Second)?.Id == session.Id) _sessions.Remove(session.Second);
    }
}
