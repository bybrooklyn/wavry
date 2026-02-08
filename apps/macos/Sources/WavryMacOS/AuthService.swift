import Foundation
import Combine

struct LoginResponse: Decodable {
    let token: String
    let user_id: String
    let email: String
    let totp_required: Bool
    let username: String?

    private struct SessionPayload: Codable {
        let token: String
    }

    private struct UserPayload: Codable {
        let id: String
        let email: String
        let username: String?
    }

    private enum CodingKeys: String, CodingKey {
        case token
        case user_id
        case email
        case username
        case totp_required
        case user
        case session
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)

        let directToken = try container.decodeIfPresent(String.self, forKey: .token)
        let directUserId = try container.decodeIfPresent(String.self, forKey: .user_id)
        let directEmail = try container.decodeIfPresent(String.self, forKey: .email)
        let directUsername = try container.decodeIfPresent(String.self, forKey: .username)
        if let directToken, let directUserId, let directEmail {
            token = directToken
            user_id = directUserId
            email = directEmail
            username = directUsername
            totp_required = try container.decodeIfPresent(Bool.self, forKey: .totp_required) ?? false
            return
        }

        let nestedSession = try container.decodeIfPresent(SessionPayload.self, forKey: .session)
        let nestedUser = try container.decodeIfPresent(UserPayload.self, forKey: .user)
        if let nestedSession, let nestedUser {
            token = nestedSession.token
            user_id = nestedUser.id
            email = nestedUser.email
            username = nestedUser.username
            totp_required = false
            return
        }

        throw DecodingError.dataCorruptedError(
            forKey: .token,
            in: container,
            debugDescription: "Unsupported auth response payload."
        )
    }
}

struct ErrorResponse: Codable {
    let error: String
}

class AuthService {
    static let shared = AuthService()
    
    private init() {}
    
    func login(server: String, email: String, password: String) async throws -> LoginResponse {
        guard let url = URL(string: "\(server)/auth/login") else {
            throw URLError(.badURL)
        }
        
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        
        let body: [String: String] = [
            "email": email,
            "password": password
        ]
        
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        
        let (data, response) = try await URLSession.shared.data(for: request)
        
        if let httpResponse = response as? HTTPURLResponse, !(200...299).contains(httpResponse.statusCode) {
            if let errResp = try? JSONDecoder().decode(ErrorResponse.self, from: data) {
                throw NSError(domain: "AuthService", code: httpResponse.statusCode, userInfo: [NSLocalizedDescriptionKey: errResp.error])
            }
            throw NSError(domain: "AuthService", code: httpResponse.statusCode, userInfo: [NSLocalizedDescriptionKey: "Server error"])
        }
        
        return try JSONDecoder().decode(LoginResponse.self, from: data)
    }
    
    func register(server: String, email: String, password: String, username: String, publicKey: String) async throws {
        guard let url = URL(string: "\(server)/auth/register") else {
            throw URLError(.badURL)
        }
        
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        
        let body: [String: String] = [
            "email": email,
            "password": password,
            "username": username,
            "display_name": username, // Default display name to username
            "public_key": publicKey
        ]
        
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        
        let (data, response) = try await URLSession.shared.data(for: request)
        
        if let httpResponse = response as? HTTPURLResponse, !(200...299).contains(httpResponse.statusCode) {
             if let errResp = try? JSONDecoder().decode(ErrorResponse.self, from: data) {
                throw NSError(domain: "AuthService", code: httpResponse.statusCode, userInfo: [NSLocalizedDescriptionKey: errResp.error])
            }
            throw NSError(domain: "AuthService", code: httpResponse.statusCode, userInfo: [NSLocalizedDescriptionKey: "Registration failed"])
        }
    }
}
