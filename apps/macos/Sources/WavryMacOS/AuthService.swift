import Foundation
import Combine

struct LoginResponse: Codable {
    let token: String
    let user_id: String
    let email: String
    let totp_required: Bool
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
