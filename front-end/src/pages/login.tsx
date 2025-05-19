import { useState } from 'react'
import { useNavigate, useLocation } from 'react-router-dom'
import { Logo } from '../components/logo'
import { Button } from '../components/button'
import { Checkbox, CheckboxField } from '../components/checkbox'
import { Field, Label } from '../components/fieldset'
import { Heading } from '../components/heading'
import { Input } from '../components/input'
import { Strong, Text, TextLink } from '../components/text'
import { useAuth } from '../contexts/AuthContext'

export default function Login() {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [serverUrl, setServerUrl] = useState('')
  const [remember, setRemember] = useState(false)
  const [error, setError] = useState('')
  const [isLoading, setIsLoading] = useState(false)

  const { login, setApiBaseUrl } = useAuth()
  const navigate = useNavigate()
  const location = useLocation()

  // Get the 'from' URL from the location state, or default to '/'
  const from = location.state?.from?.pathname || '/'

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()

    if (!username || !password || !serverUrl) {
      setError('Username, password, and server URL are required')
      return
    }

    // Store the server URL for API requests
    setApiBaseUrl(serverUrl)

    setIsLoading(true)
    setError('')

    try {
      console.log('Attempting login with:', { username, serverUrl })
      const user = await login(username, password, remember)
      console.log('Login successful:', user)

      // Debug the navigation target
      console.log('Navigating to:', from)

      // Force navigation to home page
      navigate('/', { replace: true })
    } catch (err) {
      console.error('Login error:', err)
      setError(err instanceof Error ? err.message : 'Login failed')
    } finally {
      setIsLoading(false)
    }
  }

  return (
    <form onSubmit={handleSubmit} className="grid w-full max-w-sm grid-cols-1 gap-8">
      <Logo className="h-6 text-zinc-950 dark:text-white forced-colors:text-[CanvasText]" />
      <Heading>Sign in to your account</Heading>

      {error && (
        <div className="bg-red-50 border border-red-200 text-red-800 px-4 py-3 rounded">
          {error}
        </div>
      )}

      <Field>
        <Label>Server URL</Label>
        <Input
          type="text"
          name="serverUrl"
          value={serverUrl}
          onChange={(e) => setServerUrl(e.target.value)}
          placeholder="http://localhost:8080"
          required
        />
      </Field>

      <Field>
        <Label>Username</Label>
        <Input
          type="text"
          name="username"
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          required
        />
      </Field>
      <Field>
        <Label>Password</Label>
        <Input
          type="password"
          name="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          required
        />
      </Field>
      <div className="flex items-center justify-between">
        <CheckboxField>
          <Checkbox
            name="remember"
            checked={remember}
            onChange={(e) => setRemember(e.target.checked)}
          />
          <Label>Remember me</Label>
        </CheckboxField>
        <Text>
          <TextLink href="/forgot-password">
            <Strong>Forgot password?</Strong>
          </TextLink>
        </Text>
      </div>
      <Button
        type="submit"
        className="w-full"
        disabled={isLoading}
      >
        {isLoading ? 'Logging in...' : 'Login'}
      </Button>
      <Text>
        Don't have an account?{' '}
        <TextLink href="/register">
          <Strong>Sign up</Strong>
        </TextLink>
      </Text>
    </form>
  )
}
