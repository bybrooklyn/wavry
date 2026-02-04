export class AppState {
    displayName = $state("My Desktop");
    connectivityMode = $state("Wavry Service");
    isConnected = $state(false);
    hasPermissions = $state(true); // Mocked for now

    connect() {
        this.isConnected = true;
    }

    disconnect() {
        this.isConnected = false;
    }

    get effectiveDisplayName() {
        return this.displayName || "Local Host";
    }
}

export const appState = new AppState();
